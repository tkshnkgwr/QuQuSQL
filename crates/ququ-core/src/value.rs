use compact_str::CompactString;
use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::Arc;

/// 省メモリセル表現。
///
/// Oracle `NUMBER` は 16 桁以上の数値 ID や金額で浮動小数点丸め誤差が発生するため、
/// `Decimal(CompactString)` で厳密保持する。
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Decimal(CompactString),
    Text(CompactString),
    DateTime(CompactString),
    Bytes(Arc<[u8]>),
}

impl CellValue {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Float(_) | Self::Decimal(_))
    }

    /// 描画や比較向けの文字列スライス／借用文字列を返す。
    pub fn display_str(&self) -> Cow<'_, str> {
        match self {
            Self::Null => Cow::Borrowed("<NULL>"),
            Self::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
            Self::Int(i) => Cow::Owned(i.to_string()),
            Self::Float(f) => Cow::Owned(format_float(*f)),
            Self::Decimal(s) | Self::Text(s) | Self::DateTime(s) => Cow::Borrowed(s.as_str()),
            Self::Bytes(bytes) => {
                let preview_len = bytes.len().min(32);
                let mut hex = String::with_capacity(preview_len * 2 + 16);
                hex.push_str("<BLOB ");
                let _ = write!(hex, "{}B: ", bytes.len());
                for b in &bytes[..preview_len] {
                    let _ = write!(hex, "{b:02X}");
                }
                if bytes.len() > preview_len {
                    hex.push_str("...");
                }
                hex.push('>');
                Cow::Owned(hex)
            }
        }
    }

    /// クリップボードコピー等の実値表現（NULL は空文字列）。
    pub fn copy_str(&self) -> Cow<'_, str> {
        match self {
            Self::Null => Cow::Borrowed(""),
            _ => self.display_str(),
        }
    }

    /// 再利用可能なバッファへ書き込むことで、仮想グリッド描画時のヒープ割り当てを抑える。
    pub fn write_display_into(&self, buf: &mut String) {
        buf.clear();
        match self {
            Self::Null => buf.push_str("<NULL>"),
            Self::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
            Self::Int(i) => {
                let _ = write!(buf, "{i}");
            }
            Self::Float(f) => {
                let _ = write!(buf, "{}", format_float(*f));
            }
            Self::Decimal(s) | Self::Text(s) | Self::DateTime(s) => buf.push_str(s.as_str()),
            Self::Bytes(bytes) => {
                let _ = write!(buf, "<BLOB {}B>", bytes.len());
            }
        }
    }
}

fn format_float(f: f64) -> String {
    if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_preserves_high_precision_numbers() {
        let large_id = "1234567890123456789012345";
        let cell = CellValue::Decimal(CompactString::from(large_id));
        assert_eq!(cell.display_str().as_ref(), large_id);
        assert!(cell.is_numeric());
        assert!(!cell.is_null());
    }

    #[test]
    fn null_and_empty_text_are_distinct() {
        let null_cell = CellValue::Null;
        let empty_cell = CellValue::Text(CompactString::new(""));
        assert!(null_cell.is_null());
        assert!(!empty_cell.is_null());
        assert_eq!(null_cell.display_str().as_ref(), "<NULL>");
        assert_eq!(empty_cell.display_str().as_ref(), "");
    }
}
