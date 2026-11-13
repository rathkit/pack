//! A browserslist input for `--target`: the queries are read from the project (the `BROWSERSLIST` variable, a `.browserslistrc` or
//! `browserslist` file, the `browserslist` key of `package.json`) and turned into a target (`chrome80,ie11`).
//!
//! Only the queries that name engines and versions are understood (`ie 11`, `chrome >= 80`, `safari 13.1-14`, `not ie <= 11`). The ones
//! that need usage statistics or release dates (`> 0.5%`, `last 2 versions`, `defaults`, `dead`) stop with a message that names the query:
//! this program ships no such data, and guessing it would make a different build from the one the author asked for.
use std::path::{Path, PathBuf};

use crate::json::Json;

/// The queries found for a project and where they came from.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub queries: Vec<String>,
    pub origin: String,
}

/// The queries of the project that holds `dir`, or `None` when there are none. `env` is the browserslist environment (`production` by
/// default); `variable` the value of `BROWSERSLIST`, which wins over any file.
pub fn read_project(dir: &Path, env: &str, variable: Option<&str>) -> Result<Option<Found>, String> {
    if let Some(v) = variable.filter(|v| !v.trim().is_empty()) {
        return Ok(Some(Found { queries: split_queries(v), origin: "the BROWSERSLIST variable".to_string() }));
    }
    let mut current: Option<PathBuf> = Some(dir.to_path_buf());
    while let Some(d) = current {
        for name in [".browserslistrc", "browserslist"] {
            let path = d.join(name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                let queries = parse_config(&text, env);
                if queries.is_empty() {
                    continue;
                }
                return Ok(Some(Found { queries, origin: path.display().to_string() }));
            }
        }
        let package = d.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&package) {
            if let Ok(json) = Json::parse(&text) {
                if let Some(value) = json.get("browserslist") {
                    let queries = from_json(value, env);
                    if !queries.is_empty() {
                        return Ok(Some(Found { queries, origin: format!("{} (browserslist)", package.display()) }));
                    }
                }
            }
        }
        current = d.parent().map(Path::to_path_buf);
    }
    Ok(None)
}

fn from_json(value: &Json, env: &str) -> Vec<String> {
    match value {
        Json::String(s) => split_queries(s),
        Json::Array(items) => items.iter().filter_map(|i| i.as_str()).flat_map(split_queries).collect(),
        Json::Object(_) => {
            let pick = value.get(env).or_else(|| value.get("defaults"));
            pick.map(|p| from_json(p, env)).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// The text of a `.browserslistrc`: one query a line, `#` starts a comment, `[env]` starts the queries of an environment.
fn parse_config(text: &str, env: &str) -> Vec<String> {
    let mut sections: Vec<(String, Vec<String>)> = vec![("defaults".to_string(), Vec::new())];
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim().to_string(), Vec::new()));
        } else {
            sections.last_mut().unwrap().1.extend(split_queries(line));
        }
    }
    let by = |name: &str| sections.iter().find(|(n, _)| n == name).map(|(_, q)| q.clone()).filter(|q| !q.is_empty());
    by(env).or_else(|| by("defaults")).unwrap_or_default()
}

fn split_queries(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in text.split(['\n', ',']) {
        let part = part.split('#').next().unwrap_or("");
        // ` or ` is a comma.
        for q in part.split(" or ") {
            let q = q.trim();
            if !q.is_empty() {
                out.push(q.to_string());
            }
        }
    }
    out
}

/// A target (`chrome80,ie11`) that has the engines the queries name, each at its lowest version.
pub fn to_target(queries: &[String]) -> Result<String, String> {
    // Engine and lowest version, in the order first named.
    let mut found: Vec<(String, (u32, u32), String)> = Vec::new();
    let mut ie_removed = false;
    let mut positive = 0;
    for query in queries {
        let lower = query.to_ascii_lowercase();
        let (negated, body) = match lower.strip_prefix("not ") {
            Some(rest) => (true, rest.trim().to_string()),
            None => (false, lower.clone()),
        };
        if body.contains(" and ") {
            return Err(format!("the query '{query}' combines queries with `and`, which needs data this program does not have; name the engines and versions instead"));
        }
        let words: Vec<&str> = body.split_whitespace().collect();
        let data = |why: &str| Err(format!("the query '{query}' needs {why}, which Rathkit Pack does not ship; name the engines and versions instead (`chrome >= 80, safari >= 13, ie 11`)"));
        if body == "defaults" || body == "dead" || body.starts_with("last ") || body.starts_with('>') || body.starts_with('<') || body.starts_with("cover") || body.starts_with("since") || body.starts_with("supports") || body.starts_with("fully supports") || body.starts_with("extends") || body.starts_with("unreleased") || body.starts_with("maintained") || body.starts_with("current") || body.starts_with("browserslist config") || body.contains("esr") {
            if negated {
                // Taking a set away from the engines can only make the output keep more syntax lowered; it is safe to leave out.
                continue;
            }
            return data("usage statistics or release dates");
        }
        if words.len() < 2 {
            return Err(format!("the query '{query}' is not one this program understands (`ie 11`, `chrome >= 80`, `safari 13.1-14`)"));
        }
        let name = words[0];
        let rest: String = words[1..].join("");
        let engine = match name {
            "chrome" | "and_chr" => Some("chrome"),
            "firefox" | "and_ff" => Some("firefox"),
            "safari" => Some("safari"),
            "ios_saf" => Some("ios"),
            "edge" => Some("edge"),
            "ie" | "ie_mob" => Some("ie"),
            "node" => Some("node"),
            "opera" | "op_mob" => Some("opera"),
            "android" => Some("android"),
            _ => None,
        };
        let Some(engine) = engine else {
            if negated {
                continue;
            }
            return Err(format!("the engine '{name}' in the query '{query}' is not one this program knows (chrome, edge, firefox, safari, ios_saf, ie, node, opera, android)"));
        };
        if negated {
            if engine == "ie" {
                ie_removed = true;
                continue;
            }
            return data("release data to take versions away");
        }
        positive += 1;
        let version_text = rest.trim_start_matches(">=").trim_start_matches('>');
        if rest.starts_with('<') {
            return data("release data (the oldest version of the engine)");
        }
        let first = version_text.split('-').next().unwrap_or("");
        if first == "all" {
            return data("release data");
        }
        let mut parts = first.split('.');
        let major = parts.next().and_then(|n| n.parse::<u32>().ok());
        let minor = parts.next().map_or(Some(0), |n| n.parse::<u32>().ok());
        let (Some(major), Some(minor)) = (major, minor) else {
            return Err(format!("'{}' in the query '{query}' is not a version", first));
        };
        let (engine, version) = match engine {
            // An Android browser before 37 is the old WebKit one, with none of es2015; from 37 it is Chrome.
            "android" if major < 37 => ("ie", (11, 0)),
            "android" => ("chrome", (major, minor)),
            other => (other, (major, minor)),
        };
        match found.iter_mut().find(|(e, _, _)| e == engine) {
            Some(entry) => {
                if version < entry.1 {
                    entry.1 = version;
                    entry.2 = first.to_string();
                }
            }
            None => found.push((engine.to_string(), version, first.to_string())),
        }
    }
    if ie_removed {
        found.retain(|(e, _, _)| e != "ie");
    }
    if positive == 0 || found.is_empty() {
        return Err("the browserslist queries name no engine and version this program can use".to_string());
    }
    Ok(found.iter().map(|(e, _, v)| if e == "ie" { "ie11".to_string() } else { format!("{e}{v}") }).collect::<Vec<_>>().join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(text: &str) -> Result<String, String> {
        to_target(&split_queries(text))
    }

    #[test]
    fn explicit_engines() {
        assert_eq!(target("ie 11, chrome >= 80").unwrap(), "ie11,chrome80");
        assert_eq!(target("safari 13.1-14, ios_saf 12.2").unwrap(), "safari13.1,ios12.2");
        assert_eq!(target("chrome 90 or chrome 80").unwrap(), "chrome80");
        assert_eq!(target("android 4.4, firefox 78").unwrap(), "ie11,firefox78");
        assert_eq!(target("ie 11, not ie <= 11, chrome 100").unwrap(), "chrome100");
    }

    #[test]
    fn queries_that_need_data_are_refused() {
        for q in ["defaults", "> 0.5%", "last 2 versions", "chrome < 50", "dead"] {
            assert!(target(q).unwrap_err().contains("Rathkit Pack does not ship") || q == "dead", "{q}");
        }
        assert!(target("netscape 4").unwrap_err().contains("not one this program knows"));
    }

    #[test]
    fn config_files_have_sections_and_comments() {
        let text = "# all\nchrome 80\n[legacy]\nie 11 # old\n[production]\nsafari 13\n";
        assert_eq!(parse_config(text, "production"), vec!["safari 13"]);
        assert_eq!(parse_config(text, "legacy"), vec!["ie 11"]);
        assert_eq!(parse_config(text, "development"), vec!["chrome 80"]);
    }
}
