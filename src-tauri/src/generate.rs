//! Files the AI makes for the user (make_file tool): text and code, Markdown, Word, PDF and ZIP.
//!
//! The AI runs in a read-only sandbox, so it cannot write files itself. Instead it sends the
//! content to this tool, which renders it and saves it under one folder
//! (`Downloads/Canvas Assistant/AI 생성`). Names are sanitized, nothing is written outside that
//! folder, existing files are never overwritten, and executable formats are refused.
use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "AI 생성";
const MAX_TEXT: usize = 5 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 200;

/// Formats that run code when opened. These are refused outright.
const BLOCKED: &[&str] = &[
    "exe", "com", "msi", "msp", "scr", "pif", "lnk", "url", "dll", "sys", "cpl", "reg", "hta", "inf", "app", "pkg", "dmg", "deb", "rpm",
    "apk", "ipa", "jar", "appx", "msix", "workflow", "scpt", "applescript",
];
/// Formats the app opens directly with the default app. Others (scripts, code) are only revealed in the folder.
const SAFE_OPEN: &[&str] = &[
    "pdf", "docx", "txt", "md", "markdown", "csv", "tsv", "json", "xml", "yaml", "yml", "log", "zip", "png", "jpg", "jpeg", "svg",
    "c", "h", "cpp", "hpp", "cc", "java", "py", "ipynb", "rs", "go", "kt", "swift", "m", "tex", "bib", "sql", "css", "ts", "tsx", "jsx", "r",
];

pub fn extension(name: &str) -> String {
    crate::extract::extension(name)
}

pub fn safe_to_open(name: &str) -> bool {
    SAFE_OPEN.contains(&extension(name).as_str())
}

/// File name without folders or characters Windows/macOS reject.
pub fn clean_name(name: &str) -> Result<String, String> {
    let base = name.trim().rsplit(['/', '\\']).next().unwrap_or("").trim();
    let cleaned: String = base.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() || !cleaned.contains('.') {
        return Err("name에 확장자를 포함한 파일 이름을 주세요 (예: 보고서.pdf, main.py, 과제.zip)".into());
    }
    let stem = cleaned.rsplit_once('.').map(|(s, _)| s.to_ascii_uppercase()).unwrap_or_default();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "LPT1" | "LPT2" | "LPT3") {
        return Err("사용할 수 없는 파일 이름입니다".into());
    }
    let ext = extension(&cleaned);
    if BLOCKED.contains(&ext.as_str()) {
        return Err(format!(".{ext} 파일은 실행 파일이라 만들 수 없습니다."));
    }
    Ok(cleaned.chars().take(120).collect())
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((name.to_string(), String::new()));
    (1..1000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Renders one file's bytes from text content, choosing the format by extension.
pub fn render(name: &str, content: &str, title: Option<&str>) -> Result<Vec<u8>, String> {
    if content.len() > MAX_TEXT {
        return Err("내용이 너무 깁니다 (5MB 이하)".into());
    }
    match extension(name).as_str() {
        "pdf" => pdf(content, title.unwrap_or_else(|| name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name))),
        "docx" => docx(content),
        "doc" | "hwp" | "ppt" | "xls" | "pptx" | "xlsx" | "hwpx" => {
            Err(format!(".{} 형식은 만들 수 없습니다. .docx, .pdf, .md 또는 .csv로 만들어 주세요.", extension(name)))
        }
        "zip" => Err("zip은 files 목록으로 만드세요".into()),
        _ => {
            // Windows editors read UTF-8 CSV correctly only with a BOM.
            let mut bytes = Vec::new();
            if matches!(extension(name).as_str(), "csv" | "tsv") {
                bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            bytes.extend_from_slice(content.as_bytes());
            Ok(bytes)
        }
    }
}

/// ZIP entry path: relative, no `..`, forward slashes.
fn entry_path(path: &str) -> Result<String, String> {
    let parts: Vec<String> = path.replace('\\', "/").split('/').map(str::trim).filter(|p| !p.is_empty() && *p != ".").map(|p| {
        if p == ".." { Err("압축 파일 안 경로에 ..를 쓸 수 없습니다".to_string()) } else { Ok(p.chars().map(|c| if r#"<>:"|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect()) }
    }).collect::<Result<_, _>>()?;
    let joined = parts.join("/");
    if joined.is_empty() {
        return Err("압축 파일 안 파일 경로가 비어 있습니다".into());
    }
    let ext = extension(&joined);
    if BLOCKED.contains(&ext.as_str()) {
        return Err(format!("압축 파일에 실행 파일(.{ext})은 넣을 수 없습니다"));
    }
    Ok(joined)
}

fn zip(files: &[Value]) -> Result<Vec<u8>, String> {
    if files.is_empty() {
        return Err("files가 비어 있습니다".into());
    }
    if files.len() > MAX_ZIP_ENTRIES {
        return Err(format!("압축 파일에는 {MAX_ZIP_ENTRIES}개까지 넣을 수 있습니다"));
    }
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut seen = std::collections::HashSet::new();
        let mut total = 0usize;
        for f in files {
            let path = entry_path(f.get("path").and_then(Value::as_str).unwrap_or(""))?;
            if !seen.insert(path.to_lowercase()) {
                return Err(format!("압축 파일 안에 같은 경로가 두 번 있습니다: {path}"));
            }
            let content = f.get("content").and_then(Value::as_str).unwrap_or("");
            total += content.len();
            if total > MAX_TEXT * 4 {
                return Err("압축할 내용이 너무 큽니다".into());
            }
            let bytes = render(&path, content, None)?;
            z.start_file(path.as_str(), opts).map_err(|e| e.to_string())?;
            z.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

/// make_file: renders and saves the file, returning where it went.
pub fn make_file(out_dir: &Path, args: &Value) -> Result<Value, String> {
    let name = clean_name(text_arg(args, "name").unwrap_or(""))?;
    let bytes = if extension(&name) == "zip" {
        zip(args.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]))?
    } else {
        let content = text_arg(args, "content").ok_or("content(파일 내용)가 필요합니다")?;
        render(&name, content, text_arg(args, "title"))?
    };
    std::fs::create_dir_all(out_dir).map_err(|e| format!("저장 폴더를 만들 수 없습니다: {e}"))?;
    let path = unique_path(out_dir, &name);
    // create_new: never replace an existing file, even if one appeared in between.
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("파일을 저장할 수 없습니다: {e}"))?;
    f.write_all(&bytes).map_err(|e| format!("파일을 저장할 수 없습니다: {e}"))?;
    let saved = path.file_name().and_then(|n| n.to_str()).unwrap_or(&name).to_string();
    Ok(json!({
        "saved": true,
        "name": saved,
        "path": path.to_string_lossy(),
        "size": bytes.len(),
        "note": "파일을 만들었습니다. 사용자 화면의 대화에 '열기'와 '폴더에서 보기' 버튼이 표시되니, 경로를 길게 설명하지 말고 파일 이름만 알려주세요."
    }))
}

/// True when `path` is inside `dir` (both canonicalized).
pub fn is_inside(dir: &Path, path: &Path) -> bool {
    match (dir.canonicalize(), path.canonicalize()) {
        (Ok(d), Ok(p)) => p.starts_with(&d) && p != d,
        _ => false,
    }
}

// ---------------- Markdown-ish blocks shared by PDF and DOCX ----------------

#[derive(Debug, PartialEq)]
enum Block {
    Heading(u8, String),
    Para(String),
    Bullet(u8, String),
    Numbered(String, String),
    Code(Vec<String>),
    Quote(String),
    Rule,
    Blank,
}

/// Removes inline Markdown markers (**bold**, `code`, [text](url)) for plain rendering.
fn inline(text: &str) -> String {
    let mut s = text.replace("**", "").replace("__", "").replace('`', "");
    // [text](url) → text (url)
    while let Some(open) = s.find('[') {
        let Some(mid) = s[open..].find("](").map(|m| open + m) else { break };
        let Some(close) = s[mid..].find(')').map(|c| mid + c) else { break };
        let label = s[open + 1..mid].to_string();
        let url = s[mid + 2..close].to_string();
        s.replace_range(open..=close, &if url.is_empty() || url == label { label } else { format!("{label} ({url})") });
    }
    s
}

fn blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut code: Option<Vec<String>> = None;
    for raw in md.replace("\r\n", "\n").lines() {
        if raw.trim_start().starts_with("```") {
            match code.take() {
                Some(lines) => out.push(Block::Code(lines)),
                None => code = Some(Vec::new()),
            }
            continue;
        }
        if let Some(lines) = code.as_mut() {
            lines.push(raw.replace('\t', "    "));
            continue;
        }
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indent = ((line.len() - trimmed.len()) / 2).min(3) as u8;
        if trimmed.is_empty() {
            out.push(Block::Blank);
        } else if let Some(rest) = trimmed.strip_prefix("#") {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count() as u8;
            out.push(Block::Heading(level.min(4), inline(rest.trim_start_matches('#').trim())));
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push(Block::Rule);
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
            out.push(Block::Bullet(indent, inline(rest)));
        } else if let Some((num, rest)) = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())) {
            out.push(Block::Numbered(num.to_string(), inline(rest)));
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            out.push(Block::Quote(inline(rest.trim())));
        } else if trimmed.starts_with('|') {
            // Tables: drop the |---| separator row, keep cells separated.
            if trimmed.chars().all(|c| "|-: ".contains(c)) {
                continue;
            }
            let cells: Vec<String> = trimmed.trim_matches('|').split('|').map(|c| inline(c.trim())).collect();
            out.push(Block::Code(vec![cells.join("  |  ")]));
        } else {
            out.push(Block::Para(inline(trimmed)));
        }
    }
    if let Some(lines) = code {
        out.push(Block::Code(lines));
    }
    out
}

// ---------------- DOCX ----------------

fn xml_escape(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\t').collect::<String>()
        .replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn run(text: &str) -> String {
    let mut out = String::new();
    for (i, part) in text.split('\t').enumerate() {
        if i > 0 {
            out.push_str("<w:r><w:tab/></w:r>");
        }
        out.push_str(&format!(r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#, xml_escape(part)));
    }
    out
}

fn docx(md: &str) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    let para = |style: Option<&str>, extra: &str, text: &str| {
        let ppr = match style {
            Some(s) => format!(r#"<w:pPr><w:pStyle w:val="{s}"/>{extra}</w:pPr>"#),
            None if !extra.is_empty() => format!("<w:pPr>{extra}</w:pPr>"),
            None => String::new(),
        };
        format!("<w:p>{ppr}{}</w:p>", run(text))
    };
    for b in blocks(md) {
        match b {
            Block::Heading(l, t) => body.push_str(&para(Some(&format!("Heading{}", l.min(3))), "", &t)),
            Block::Para(t) => body.push_str(&para(None, "", &t)),
            Block::Bullet(indent, t) => body.push_str(&para(None, &format!(r#"<w:ind w:left="{}" w:hanging="240"/>"#, 360 + indent as u32 * 360), &format!("• {t}"))),
            Block::Numbered(n, t) => body.push_str(&para(None, r#"<w:ind w:left="360" w:hanging="300"/>"#, &format!("{n}. {t}"))),
            Block::Quote(t) => body.push_str(&para(Some("Quote"), "", &t)),
            Block::Code(lines) => for l in lines {
                body.push_str(&para(Some("Code"), "", &l));
            },
            Block::Rule => body.push_str(r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="999999"/></w:pBdr></w:pPr></w:p>"#),
            Block::Blank => {}
        }
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1300" w:bottom="1440" w:left="1300" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let font = r#"<w:rFonts w:ascii="Malgun Gothic" w:hAnsi="Malgun Gothic" w:eastAsia="Malgun Gothic" w:cs="Malgun Gothic"/>"#;
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr>{font}<w:sz w:val="21"/><w:lang w:val="ko-KR" w:eastAsia="ko-KR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="300" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="100"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="27"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="160" w:after="80"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="23"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="400"/></w:pPr><w:rPr><w:i/><w:color w:val="555555"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:shd w:val="clear" w:color="auto" w:fill="F3F4F6"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:eastAsia="Malgun Gothic"/><w:sz w:val="18"/></w:rPr></w:style></w:styles>"#
    );
    let files = [
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.to_string()),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_string()),
        ("word/_rels/document.xml.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_string()),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ];
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (n, c) in files {
            z.start_file(n, opts).map_err(|e| e.to_string())?;
            z.write_all(c.as_bytes()).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

// ---------------- PDF ----------------

/// TrueType fonts with Hangul that ship with the OS. `CA_PDF_FONT` overrides (used by tests).
fn font_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("CA_PDF_FONT") {
        v.push(PathBuf::from(p));
    }
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        for f in ["malgun.ttf", "NanumGothic.ttf", "gulim.ttf"] {
            v.push(windir.join("Fonts").join(f));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts\\NanumGothic.ttf"));
        }
    } else if cfg!(target_os = "macos") {
        for f in [
            "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
            "/Library/Fonts/NanumGothic.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            v.push(PathBuf::from(f));
        }
        if let Some(home) = std::env::var_os("HOME") {
            v.push(PathBuf::from(home).join("Library/Fonts/NanumGothic.ttf"));
        }
    } else {
        for f in ["/usr/share/fonts/truetype/nanum/NanumGothic.ttf", "/usr/share/fonts/nanum/NanumGothic.ttf"] {
            v.push(PathBuf::from(f));
        }
    }
    v
}

fn load_font() -> Result<Vec<u8>, String> {
    for p in font_candidates() {
        if let Ok(bytes) = std::fs::read(&p) {
            // Only glyf-based TrueType embeds correctly as FontFile2.
            if let Ok(face) = ttf_parser::Face::parse(&bytes, 0) {
                if face.glyph_index('가').is_some() && face.tables().glyf.is_some() {
                    return Ok(bytes);
                }
            }
        }
    }
    Err("PDF에 쓸 한글 글꼴(맑은 고딕/AppleGothic)을 찾지 못했습니다. .docx로 만들어 보세요.".into())
}

struct Metrics<'a> {
    face: ttf_parser::Face<'a>,
}

impl Metrics<'_> {
    /// Width in mm of `text` at `size` pt.
    fn width(&self, text: &str, size: f32) -> f32 {
        let upem = self.face.units_per_em() as f32;
        let units: f32 = text.chars().map(|c| {
            self.face.glyph_index(c).and_then(|g| self.face.glyph_hor_advance(g)).map(|a| a as f32).unwrap_or(upem * 0.5)
        }).sum();
        units / upem * size * 0.352_778
    }

    /// Greedy wrap at spaces; words longer than the line are split by character.
    fn wrap(&self, text: &str, size: f32, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in text.split(' ') {
            let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.width(&candidate, size) <= max {
                cur = candidate;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if self.width(word, size) <= max {
                cur = word.to_string();
            } else {
                for ch in word.chars() {
                    let next = format!("{cur}{ch}");
                    if self.width(&next, size) > max && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur.push(ch);
                    } else {
                        cur = next;
                    }
                }
            }
        }
        if !cur.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
        lines
    }
}

fn pdf(md: &str, title: &str) -> Result<Vec<u8>, String> {
    use printpdf::{Color, Line, Mm, PdfDocument, Point, Rgb};
    let font_bytes = load_font()?;
    let metrics = Metrics { face: ttf_parser::Face::parse(&font_bytes, 0).map_err(|e| e.to_string())? };
    let (w, h, margin) = (210.0f32, 297.0f32, 20.0f32);
    let text_w = w - margin * 2.0;
    let (doc, page, layer) = PdfDocument::new(title, Mm(w), Mm(h), "본문");
    let font = doc.add_external_font_with_subsetting(Cursor::new(font_bytes.clone()), true).map_err(|e| format!("글꼴을 넣지 못했습니다: {e}"))?;
    let mut layer_ref = doc.get_page(page).get_layer(layer);
    let mut y = h - margin;
    let new_page = |doc: &printpdf::PdfDocumentReference| {
        let (p, l) = doc.add_page(Mm(w), Mm(h), "본문");
        doc.get_page(p).get_layer(l)
    };
    let black = Color::Rgb(Rgb::new(0.1, 0.1, 0.12, None));
    let gray = Color::Rgb(Rgb::new(0.35, 0.35, 0.4, None));
    layer_ref.set_fill_color(black.clone());

    // (text, size, indent mm, color, space before mm, hanging prefix)
    let emit = |layer_ref: &mut printpdf::PdfLayerReference, y: &mut f32, text: &str, size: f32, indent: f32, color: &Color, before: f32, prefix: &str| {
        let line_h = size * 0.352_778 * 1.55;
        *y -= before;
        let prefix_w = if prefix.is_empty() { 0.0 } else { metrics.width(prefix, size) + 1.2 };
        let lines = metrics.wrap(text, size, text_w - indent - prefix_w);
        for (i, line) in lines.iter().enumerate() {
            if *y - line_h < margin {
                *layer_ref = new_page(&doc);
                *y = h - margin;
            }
            *y -= line_h;
            layer_ref.set_fill_color(color.clone());
            if i == 0 && !prefix.is_empty() {
                layer_ref.use_text(prefix, size, Mm(margin + indent), Mm(*y), &font);
            }
            if !line.is_empty() {
                layer_ref.use_text(line.as_str(), size, Mm(margin + indent + prefix_w), Mm(*y), &font);
            }
        }
    };

    for b in blocks(md) {
        match b {
            Block::Heading(l, t) => {
                let size = match l { 1 => 18.0, 2 => 14.5, 3 => 12.5, _ => 11.5 };
                emit(&mut layer_ref, &mut y, &t, size, 0.0, &black, if l == 1 { 3.0 } else { 2.5 }, "");
                y -= 1.0;
            }
            Block::Para(t) => emit(&mut layer_ref, &mut y, &t, 10.5, 0.0, &black, 1.2, ""),
            Block::Bullet(indent, t) => emit(&mut layer_ref, &mut y, &t, 10.5, 3.0 + indent as f32 * 5.0, &black, 0.6, "•"),
            Block::Numbered(n, t) => emit(&mut layer_ref, &mut y, &t, 10.5, 3.0, &black, 0.6, &format!("{n}.")),
            Block::Quote(t) => emit(&mut layer_ref, &mut y, &t, 10.5, 6.0, &gray, 1.0, ""),
            Block::Code(lines) => {
                y -= 1.0;
                for l in lines {
                    // Keep code indentation: leading spaces become extra indent.
                    let lead = l.len() - l.trim_start_matches(' ').len();
                    let extra = metrics.width(&" ".repeat(lead), 9.0);
                    emit(&mut layer_ref, &mut y, l.trim_start_matches(' '), 9.0, 4.0 + extra, &gray, 0.0, "");
                }
                y -= 1.0;
            }
            Block::Rule => {
                y -= 3.0;
                layer_ref.set_outline_color(gray.clone());
                layer_ref.set_outline_thickness(0.5);
                layer_ref.add_line(Line { points: vec![(Point::new(Mm(margin), Mm(y)), false), (Point::new(Mm(w - margin), Mm(y)), false)], is_closed: false });
                y -= 3.0;
            }
            Block::Blank => y -= 1.5,
        }
    }
    doc.save_to_bytes().map_err(|e| format!("PDF를 만들지 못했습니다: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-gen-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn names_are_sanitized_and_executables_refused() {
        assert_eq!(clean_name("../../보고서:최종.pdf").unwrap(), "보고서_최종.pdf");
        assert_eq!(clean_name("C:\\Windows\\evil.txt").unwrap(), "evil.txt");
        assert!(clean_name("run.exe").is_err());
        assert!(clean_name("x.LNK").is_err());
        assert!(clean_name("noext").is_err());
        assert!(clean_name("CON.txt").is_err());
        assert!(safe_to_open("a.pdf") && !safe_to_open("a.bat") && !safe_to_open("a.ps1"));
    }

    #[test]
    fn saves_text_without_overwriting() {
        let d = dir();
        let a = make_file(&d, &json!({"name": "main.py", "content": "print('안녕')\n"})).unwrap();
        let b = make_file(&d, &json!({"name": "main.py", "content": "x"})).unwrap();
        assert_eq!(a["name"], "main.py");
        assert_eq!(b["name"], "main (1).py");
        assert_eq!(std::fs::read_to_string(d.join("main.py")).unwrap(), "print('안녕')\n");
        let csv = make_file(&d, &json!({"name": "점수.csv", "content": "이름,점수\n"})).unwrap();
        assert!(std::fs::read(csv["path"].as_str().unwrap()).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn builds_zip_with_nested_documents() {
        let d = dir();
        let r = make_file(&d, &json!({"name": "과제.zip", "files": [
            {"path": "src/main.cpp", "content": "int main(){}"},
            {"path": "보고서.docx", "content": "# 제목\n본문"},
        ]})).unwrap();
        let bytes = std::fs::read(r["path"].as_str().unwrap()).unwrap();
        let text = crate::extract::extract_text("과제.zip", &bytes).unwrap();
        assert!(text.contains("===== src/main.cpp =====\nint main(){}"), "{text}");
        assert!(text.contains("제목\n본문"), "{text}");
        assert!(make_file(&d, &json!({"name": "x.zip", "files": [{"path": "../a.txt", "content": ""}]})).is_err());
        assert!(make_file(&d, &json!({"name": "x.zip", "files": [{"path": "a/run.exe", "content": ""}]})).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn docx_round_trips_through_reader() {
        let bytes = docx("# 1장 서론\n- 첫째 & 둘째\n1. 번호\n```\nint x = 1;\n```\n| 이름 | 점수 |\n|---|---|\n| 홍 | 95 |").unwrap();
        let text = crate::extract::extract_text("r.docx", &bytes).unwrap();
        assert_eq!(text, "1장 서론\n• 첫째 & 둘째\n1. 번호\nint x = 1;\n이름  |  점수\n홍  |  95");
    }

    #[test]
    fn markdown_blocks() {
        let b = blocks("## 제목 **굵게**\n[링크](https://x.y)\n  - 하위");
        assert_eq!(b, vec![Block::Heading(2, "제목 굵게".into()), Block::Para("링크 (https://x.y)".into()), Block::Bullet(1, "하위".into())]);
    }

    /// Needs a Hangul TrueType font: CA_PDF_FONT=/path/NanumGothic.ttf cargo test pdf_ -- --ignored
    #[test]
    #[ignore]
    fn pdf_with_korean_text_round_trips() {
        let long = "열전달 과제 보고서의 본문입니다. ".repeat(80);
        let bytes = render("보고서.pdf", &format!("# 열전달 HW6\n\n{long}\n\n- 핀 효율 계산\n```\nq = -k dT/dx\n```"), None).unwrap();
        assert!(bytes.len() < 400_000, "subset font should keep the PDF small: {}", bytes.len());
        let text = crate::extract::extract_text("보고서.pdf", &bytes).unwrap();
        assert!(text.contains("열전달 HW6") && text.contains("핀 효율 계산"), "{text}");
        std::fs::write(std::env::temp_dir().join("ca-test.pdf"), &bytes).unwrap();
    }
}

#[cfg(test)]
mod manual {
    /// GEN_OUT=/dir cargo test gen_sample -- --ignored
    #[test]
    #[ignore]
    fn gen_sample() {
        let out = std::path::PathBuf::from(std::env::var("GEN_OUT").unwrap());
        let md = "# 열전달 HW6 정리\n\n## 1. 문제 요약\n핀의 열전달률과 효율을 구한다. **굵게**와 `코드`도 있다.\n\n- 단면적 A = πD²/4\n  - 하위 항목\n1. 경계조건 확인\n2. 효율 계산\n\n> 단위를 꼭 표기할 것\n\n| 항목 | 값 |\n|---|---|\n| k | 200 W/m·K |\n\n```\ndef eta(m, L):\n    return tanh(m*L)/(m*L)\n```\n---\n끝.";
        super::make_file(&out, &serde_json::json!({"name": "정리.docx", "content": md})).unwrap();
        super::make_file(&out, &serde_json::json!({"name": "정리.pdf", "content": md})).unwrap();
    }
}
