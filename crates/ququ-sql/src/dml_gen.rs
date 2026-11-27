//! OSqlGrid 向けのパラメータ化 DML（DELETE / UPDATE / INSERT）生成モジュール。
//!
//! [`GridPatch`] に蓄積された差分と [`EditableTableData`] のメタデータから、
//! インジェクション耐性のあるバインドパラメータ付き DML と確認ダイアログ用プレビュー SQL を生成する。

use std::fmt::Write;

use ququ_core::{
    CellValue, EditableTableData, GridPatch, ParameterizedDml, RowKey, SqlDialect,
    format_sql_literal, quote_identifier,
};

/// プレースホルダ文字列（SQLite は `?n`、Oracle は `:n`）を返す。
fn placeholder(dialect: SqlDialect, one_based_idx: usize) -> String {
    match dialect {
        SqlDialect::Sqlite => format!("?{one_based_idx}"),
        SqlDialect::Oracle => format!(":{one_based_idx}"),
    }
}

/// スキーマ修飾付きのテーブル名を生成する。
fn qualified_table(table: &EditableTableData) -> String {
    let tbl = quote_identifier(&table.table_name, table.dialect);
    if table.owner.trim().is_empty() {
        tbl
    } else {
        let own = quote_identifier(&table.owner, table.dialect);
        format!("{own}.{tbl}")
    }
}

/// 既存行の識別キーから WHERE 句（実行用 SQL・バインド値・プレビュー用文字列）を組み立てる。
fn build_row_where_clause(
    table: &EditableTableData,
    row: usize,
    start_param_idx: usize,
) -> Result<(String, Vec<CellValue>, String), String> {
    let key = table
        .row_keys
        .get(row)
        .and_then(|opt| opt.as_ref())
        .ok_or_else(|| {
            format!(
                "行 {} に ROWID / 主キーが存在しないため更新・削除 DML を生成できません",
                row + 1
            )
        })?;

    match key {
        RowKey::RowId(rowid_str) => match table.dialect {
            SqlDialect::Sqlite => {
                let ph = placeholder(SqlDialect::Sqlite, start_param_idx);
                let val = if let Ok(id_num) = rowid_str.parse::<i64>() {
                    CellValue::Int(id_num)
                } else {
                    CellValue::Text(rowid_str.as_str().into())
                };
                let preview_lit = format_sql_literal(&val, SqlDialect::Sqlite);
                Ok((
                    format!("rowid = {ph}"),
                    vec![val],
                    format!("rowid = {preview_lit}"),
                ))
            }
            SqlDialect::Oracle => {
                let ph = placeholder(SqlDialect::Oracle, start_param_idx);
                let val = CellValue::Text(rowid_str.as_str().into());
                let preview_lit = format_sql_literal(&val, SqlDialect::Oracle);
                Ok((
                    format!("ROWID = CHARTOROWID({ph})"),
                    vec![val],
                    format!("ROWID = CHARTOROWID({preview_lit})"),
                ))
            }
        },
        RowKey::PrimaryKey(pairs) => {
            if pairs.is_empty() {
                return Err(format!("行 {} の主キー定義が空です", row + 1));
            }
            let mut sql_parts = Vec::with_capacity(pairs.len());
            let mut preview_parts = Vec::with_capacity(pairs.len());
            let mut params = Vec::new();
            let mut idx = start_param_idx;

            for (col_name, val) in pairs {
                let col_ident = quote_identifier(col_name, table.dialect);
                if matches!(val, CellValue::Null) {
                    sql_parts.push(format!("{col_ident} IS NULL"));
                    preview_parts.push(format!("{col_ident} IS NULL"));
                } else {
                    let ph = placeholder(table.dialect, idx);
                    idx += 1;
                    sql_parts.push(format!("{col_ident} = {ph}"));
                    preview_parts.push(format!(
                        "{col_ident} = {}",
                        format_sql_literal(val, table.dialect)
                    ));
                    params.push(val.clone());
                }
            }
            Ok((
                sql_parts.join(" AND "),
                params,
                preview_parts.join(" AND "),
            ))
        }
        RowKey::NewRow(_) => Err(format!(
            "行 {} は新規行キーであり、既存行の WHERE 句を生成できません",
            row + 1
        )),
    }
}

/// [`EditableTableData`] と [`GridPatch`] から反映用のパラメータ化 DML リストを生成する。
///
/// 生成順は「1. 既存行の DELETE -> 2. 変更列のみ最小限の UPDATE -> 3. 新規行の INSERT」。
/// どの生成文もバインドパラメータを使用し、リテラル連結のプレビュー SQL を併せて構築する。
pub fn generate_grid_dml(
    table: &EditableTableData,
    patch: &GridPatch,
) -> Result<Vec<ParameterizedDml>, String> {
    let mut dmls = Vec::new();
    let tbl_ref = qualified_table(table);

    // 1. DELETE（削除マーク行）
    for &row in patch.deleted_rows() {
        let (where_sql, params, where_preview) = build_row_where_clause(table, row, 1)?;
        dmls.push(ParameterizedDml {
            sql: format!("DELETE FROM {tbl_ref} WHERE {where_sql}"),
            params,
            preview_sql: format!("DELETE FROM {tbl_ref} WHERE {where_preview};"),
            expect_one_row: true,
        });
    }

    // 2. UPDATE（変更された列のみを SET 句に含める）
    for (row, changed_cols) in patch.modified_cells_by_row() {
        if changed_cols.is_empty() {
            continue;
        }
        let mut set_sql_parts = Vec::with_capacity(changed_cols.len());
        let mut set_preview_parts = Vec::with_capacity(changed_cols.len());
        let mut params = Vec::with_capacity(changed_cols.len() + 1);
        let mut param_idx = 1usize;

        for (col_idx, new_val) in changed_cols {
            let col_meta = table
                .columns
                .get(col_idx)
                .ok_or_else(|| format!("不正な列インデックス {col_idx}"))?;
            let col_ident = quote_identifier(&col_meta.name, table.dialect);
            let ph = placeholder(table.dialect, param_idx);
            param_idx += 1;

            set_sql_parts.push(format!("{col_ident} = {ph}"));
            set_preview_parts.push(format!(
                "{col_ident} = {}",
                format_sql_literal(&new_val, table.dialect)
            ));
            params.push(new_val);
        }

        let (where_sql, mut where_params, where_preview) =
            build_row_where_clause(table, row, param_idx)?;
        params.append(&mut where_params);

        let set_clause = set_sql_parts.join(", ");
        let preview_set = set_preview_parts.join(", ");

        dmls.push(ParameterizedDml {
            sql: format!("UPDATE {tbl_ref} SET {set_clause} WHERE {where_sql}"),
            params,
            preview_sql: format!("UPDATE {tbl_ref} SET {preview_set} WHERE {where_preview};"),
            expect_one_row: true,
        });
    }

    // 3. INSERT（新規追加行）
    for row_vals in patch.active_inserted_rows() {
        // 非 NULL 列を優先して対象とし、全列が NULL の場合のみ全列を NULL バインドで挿入する
        let non_null_indices: Vec<usize> = row_vals
            .iter()
            .enumerate()
            .filter_map(|(idx, val)| (!matches!(val, CellValue::Null)).then_some(idx))
            .collect();

        let target_indices: Vec<usize> = if non_null_indices.is_empty() {
            (0..table.columns.len()).collect()
        } else {
            non_null_indices
        };

        if target_indices.is_empty() {
            return Err("テーブルに列定義が存在しないため INSERT を生成できません".to_string());
        }

        let mut col_idents = Vec::with_capacity(target_indices.len());
        let mut placeholders = Vec::with_capacity(target_indices.len());
        let mut preview_vals = Vec::with_capacity(target_indices.len());
        let mut params = Vec::with_capacity(target_indices.len());

        for (p_offset, &col_idx) in target_indices.iter().enumerate() {
            let col_meta = &table.columns[col_idx];
            let val = row_vals.get(col_idx).cloned().unwrap_or(CellValue::Null);
            col_idents.push(quote_identifier(&col_meta.name, table.dialect));
            placeholders.push(placeholder(table.dialect, p_offset + 1));
            preview_vals.push(format_sql_literal(&val, table.dialect));
            params.push(val);
        }

        let cols_joined = col_idents.join(", ");
        let ph_joined = placeholders.join(", ");
        let preview_joined = preview_vals.join(", ");

        dmls.push(ParameterizedDml {
            sql: format!("INSERT INTO {tbl_ref} ({cols_joined}) VALUES ({ph_joined})"),
            params,
            preview_sql: format!(
                "INSERT INTO {tbl_ref} ({cols_joined}) VALUES ({preview_joined});"
            ),
            expect_one_row: false,
        });
    }

    Ok(dmls)
}

/// 生成された DML 群の確認用プレビュー全文を 1 つの文字列に連結する。
pub fn format_dml_preview_script(dmls: &[ParameterizedDml]) -> String {
    let mut out = String::new();
    for dml in dmls {
        let _ = writeln!(out, "{}", dml.preview_sql);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ququ_core::{ColumnMeta, ResultSet};

    fn make_table(dialect: SqlDialect) -> EditableTableData {
        let cols = vec![
            ColumnMeta {
                name: "ID".into(),
                data_type: "NUMBER".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "NAME".into(),
                data_type: "VARCHAR2".into(),
                nullable: true,
            },
            ColumnMeta {
                name: "AGE".into(),
                data_type: "NUMBER".into(),
                nullable: true,
            },
        ];
        let mut rs = ResultSet::with_columns(cols.clone());
        rs.push_row(vec![
            CellValue::Int(1),
            CellValue::Text("Alice".into()),
            CellValue::Int(20),
        ]);
        rs.push_row(vec![
            CellValue::Int(2),
            CellValue::Text("Bob".into()),
            CellValue::Int(30),
        ]);

        let row_keys = match dialect {
            SqlDialect::Sqlite => vec![
                Some(RowKey::RowId("101".to_string())),
                Some(RowKey::RowId("102".to_string())),
            ],
            SqlDialect::Oracle => vec![
                Some(RowKey::RowId("AAAR7fAAEAAAAAbAAA".to_string())),
                Some(RowKey::RowId("AAAR7fAAEAAAAAbAAB".to_string())),
            ],
        };

        EditableTableData {
            dialect,
            owner: if dialect == SqlDialect::Oracle {
                "SCOTT".to_string()
            } else {
                "main".to_string()
            },
            table_name: "USERS".to_string(),
            where_clause: String::new(),
            order_by: String::new(),
            columns: cols,
            row_keys,
            result: rs,
        }
    }

    #[test]
    fn generates_minimal_column_update_and_delete_for_sqlite() {
        let table = make_table(SqlDialect::Sqlite);
        let mut patch = GridPatch::new(2, 3);
        patch.set_cell(&table, 0, 1, CellValue::Text("Alice2".into()));
        patch.toggle_delete_row(&table, 1);

        let dmls = generate_grid_dml(&table, &patch).unwrap();
        assert_eq!(dmls.len(), 2);

        // DELETE
        assert_eq!(dmls[0].sql, "DELETE FROM main.USERS WHERE rowid = ?1");
        assert_eq!(dmls[0].params, vec![CellValue::Int(102)]);
        assert!(dmls[0].expect_one_row);

        // minimal UPDATE (NAME のみ変更されているため ID と AGE は含まれない)
        assert_eq!(
            dmls[1].sql,
            "UPDATE main.USERS SET NAME = ?1 WHERE rowid = ?2"
        );
        assert_eq!(
            dmls[1].params,
            vec![CellValue::Text("Alice2".into()), CellValue::Int(101)]
        );
        assert!(dmls[1].expect_one_row);
    }

    #[test]
    fn generates_oracle_chartorowid_update_and_insert() {
        let table = make_table(SqlDialect::Oracle);
        let mut patch = GridPatch::new(2, 3);
        patch.set_cell(&table, 0, 2, CellValue::Int(21));
        let new_row = patch.insert_row(&table, None);
        patch.set_cell(&table, new_row, 0, CellValue::Int(3));
        patch.set_cell(&table, new_row, 1, CellValue::Text("Carol".into()));

        let dmls = generate_grid_dml(&table, &patch).unwrap();
        assert_eq!(dmls.len(), 2);

        assert_eq!(
            dmls[0].sql,
            "UPDATE SCOTT.USERS SET AGE = :1 WHERE ROWID = CHARTOROWID(:2)"
        );
        assert_eq!(
            dmls[0].params,
            vec![
                CellValue::Int(21),
                CellValue::Text("AAAR7fAAEAAAAAbAAA".into())
            ]
        );
        assert!(dmls[0].expect_one_row);

        assert_eq!(
            dmls[1].sql,
            "INSERT INTO SCOTT.USERS (ID, NAME) VALUES (:1, :2)"
        );
        assert_eq!(
            dmls[1].params,
            vec![CellValue::Int(3), CellValue::Text("Carol".into())]
        );
        assert!(!dmls[1].expect_one_row);
    }
}
