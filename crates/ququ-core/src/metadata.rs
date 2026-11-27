//! オブジェクトブラウザ用の DB メタデータ定義モジュール。

use serde::{Deserialize, Serialize};

/// データベースオブジェクトの種別。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectType {
    /// テーブル
    Table,
    /// ビュー
    View,
    /// シノニム
    Synonym,
    /// インデックス
    Index,
    /// シーケンス
    Sequence,
    /// トリガ
    Trigger,
    /// その他のオブジェクト種別
    Other(String),
}

impl ObjectType {
    /// DB 問い合わせおよび UI 表示向けの種別文字列を返す。
    pub fn as_str(&self) -> &str {
        match self {
            Self::Table => "TABLE",
            Self::View => "VIEW",
            Self::Synonym => "SYNONYM",
            Self::Index => "INDEX",
            Self::Sequence => "SEQUENCE",
            Self::Trigger => "TRIGGER",
            Self::Other(s) => s.as_str(),
        }
    }

    /// データベースから取得した文字列を `ObjectType` へ変換する。
    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "TABLE" => Self::Table,
            "VIEW" => Self::View,
            "SYNONYM" => Self::Synonym,
            "INDEX" => Self::Index,
            "SEQUENCE" => Self::Sequence,
            "TRIGGER" => Self::Trigger,
            other => Self::Other(other.to_string()),
        }
    }

    /// コンボボックス選択肢として表示する標準オブジェクト種別一覧を返す。
    pub fn standard_types() -> &'static [ObjectType] {
        &[
            ObjectType::Table,
            ObjectType::View,
            ObjectType::Synonym,
            ObjectType::Index,
            ObjectType::Sequence,
            ObjectType::Trigger,
        ]
    }
}

/// オブジェクト一覧ペインに表示する 1 エントリ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaObject {
    /// 所有スキーマ名
    pub owner: String,
    /// オブジェクト物理名
    pub name: String,
    /// オブジェクト種別
    pub object_type: ObjectType,
    /// 論理名・コメント
    pub comments: String,
}

/// カラム詳細ペインに表示する 1 列分の定義情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDetail {
    /// カラム物理名
    pub name: String,
    /// データ型
    pub data_type: String,
    /// 桁数・精度表記
    pub length: String,
    /// NULL 許可（`true` なら Y）
    pub nullable: bool,
    /// 主キー列かどうか
    pub is_pk: bool,
    /// カラムコメント（論理名等）
    pub comments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_object_types_case_insensitively() {
        assert_eq!(ObjectType::from_db_str("table"), ObjectType::Table);
        assert_eq!(ObjectType::from_db_str(" VIEW "), ObjectType::View);
        assert_eq!(ObjectType::from_db_str("synonym"), ObjectType::Synonym);
        assert_eq!(
            ObjectType::from_db_str("MATERIALIZED VIEW"),
            ObjectType::Other("MATERIALIZED VIEW".to_string())
        );
        assert_eq!(ObjectType::Table.as_str(), "TABLE");
    }
}
