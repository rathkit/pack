//! `rkp analyze META.json`: one HTML page from a metafile (`--metafile=FILE`), for reading what went into a build and where the bytes
//! are. The page is self-contained: its style and script are in the file and the metafile is embedded in it, so it needs no server and
//! no network. The template is `analyze.html`; the page computes everything it shows from the embedded metafile.

use crate::json::Json;

const TEMPLATE: &str = include_str!("analyze.html");
const MARKER: &str = "@@METAFILE@@";

/// The page for a metafile, or why the text is not one. A metafile is an object with `inputs` and `outputs` objects.
pub fn report(metafile: &str) -> Result<String, String> {
    let json = Json::parse(metafile).map_err(|e| format!("not JSON: {e}"))?;
    for key in ["inputs", "outputs"] {
        if json.get(key).and_then(Json::as_object).is_none() {
            return Err(format!("not a metafile: it has no \"{key}\" object (made with --metafile=FILE)"));
        }
    }
    // Written again (so that it is on one line), and kept from ending the script element or the line: `<` is only ever inside a
    // string, where \u003c means the same.
    let text = json.to_string().replace('<', "\\u003c").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
    Ok(TEMPLATE.replacen(MARKER, &text, 1))
}

#[cfg(test)]
mod tests {
    use super::report;

    #[test]
    fn a_metafile_becomes_a_page_with_it_inside() {
        let page = report(r#"{"inputs":{"src/a.js":{"bytes":5,"imports":[]}},"outputs":{"out.js":{"bytes":3,"inputs":{"src/a.js":{"bytesInOutput":3}},"imports":[],"exports":[]}}}"#).unwrap();
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("\"src/a.js\""));
        assert!(!page.contains("@@METAFILE@@"));
    }

    #[test]
    fn text_in_the_metafile_cannot_end_the_script() {
        let page = report(r#"{"inputs":{"</script><b>":{"bytes":1,"imports":[]}},"outputs":{}}"#).unwrap();
        assert!(!page.contains("</script><b>"));
        assert!(page.contains("\\u003c/script>"));
    }

    #[test]
    fn something_else_is_refused() {
        assert!(report("[]").is_err());
        assert!(report("{").is_err());
        assert!(report(r#"{"inputs":{}}"#).unwrap_err().contains("outputs"));
    }
}
