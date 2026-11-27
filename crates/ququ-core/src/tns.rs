use regex::Regex;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TnsEntry {
    pub alias: String,
    pub host: String,
    pub port: u16,
    pub service_name: Option<String>,
    pub sid: Option<String>,
}

/// `tnsnames.ora` を軽量パースし、各エントリから接続に必要な情報を抽出する。
pub fn parse_tnsnames(content: &str) -> Vec<TnsEntry> {
    let cleaned: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') { "" } else { line }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut entries = Vec::new();
    let chars: Vec<char> = cleaned.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // エイリアス名を探す（行頭・空白後の `ALIAS = (` パターン）
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        let name_start = i;
        while i < chars.len() && chars[i] != '=' && chars[i] != '(' && !chars[i].is_whitespace() {
            i += 1;
        }
        let alias: String = chars[name_start..i].iter().collect();

        // '=' までスキップ
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            // パース不能行は改行まで読み飛ばす
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        i += 1;

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '(' {
            continue;
        }

        // 対応する括弧ブロック終端までを取得
        let block_start = i;
        let mut depth = 0usize;
        while i < chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let block: String = chars[block_start..i].iter().collect();

        if let Some(entry) = extract_tns_entry(&alias, &block) {
            entries.push(entry);
        }
    }

    entries
}

fn extract_tns_entry(alias: &str, block: &str) -> Option<TnsEntry> {
    let re_host = Regex::new(r"(?i)HOST\s*=\s*([^)\s]+)").ok()?;
    let re_port = Regex::new(r"(?i)PORT\s*=\s*(\d+)").ok()?;
    let re_svc = Regex::new(r"(?i)SERVICE_NAME\s*=\s*([^)\s]+)").ok()?;
    let re_sid = Regex::new(r"(?i)SID\s*=\s*([^)\s]+)").ok()?;

    let host = re_host.captures(block)?.get(1)?.as_str().to_string();
    let port = re_port
        .captures(block)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u16>().ok())
        .unwrap_or(1521);
    let service_name = re_svc
        .captures(block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());
    let sid = re_sid
        .captures(block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let primary_alias = alias.split(',').next()?.trim().to_string();
    if primary_alias.is_empty() {
        return None;
    }

    Some(TnsEntry {
        alias: primary_alias,
        host,
        port,
        service_name,
        sid,
    })
}

/// 指定パスまたは `TNS_ADMIN` 環境変数から `tnsnames.ora` を探索する。
pub fn find_tnsnames_path(override_dir_or_file: Option<&str>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(p) = override_dir_or_file.filter(|s| !s.trim().is_empty()) {
        let path = PathBuf::from(p.trim());
        if path.is_file() {
            candidates.push(path);
        } else {
            candidates.push(path.join("tnsnames.ora"));
        }
    }

    if let Ok(tns_admin) = std::env::var("TNS_ADMIN") {
        let path = PathBuf::from(tns_admin);
        candidates.push(path.join("tnsnames.ora"));
    }

    if let Ok(oracle_home) = std::env::var("ORACLE_HOME") {
        candidates.push(
            PathBuf::from(oracle_home)
                .join("network")
                .join("admin")
                .join("tnsnames.ora"),
        );
    }

    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_tnsnames_entries() {
        let sample = r#"
# Sample comment
DEVDB =
  (DESCRIPTION =
    (ADDRESS = (PROTOCOL = TCP)(HOST = 192.168.1.10)(PORT = 1521))
    (CONNECT_DATA =
      (SERVER = DEDICATED)
      (SERVICE_NAME = DEV_SVC)
    )
  )

LEGACY_DB =
  (DESCRIPTION =
    (ADDRESS_LIST =
      (ADDRESS = (PROTOCOL = TCP)(HOST = db.example.local)(PORT = 1522))
    )
    (CONNECT_DATA =
      (SID = ORCL)
    )
  )
"#;
        let entries = parse_tnsnames(sample);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].alias, "DEVDB");
        assert_eq!(entries[0].host, "192.168.1.10");
        assert_eq!(entries[0].port, 1521);
        assert_eq!(entries[0].service_name.as_deref(), Some("DEV_SVC"));
        assert_eq!(entries[1].alias, "LEGACY_DB");
        assert_eq!(entries[1].port, 1522);
        assert_eq!(entries[1].sid.as_deref(), Some("ORCL"));
    }
}
