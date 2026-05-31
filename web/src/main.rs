//! Tiny static-site generator for rtdvi.com.
//!
//! Renders `web/content/*.md` and the project's `doc/*.md` into static HTML
//! under `web/dist/`, wrapping each page in `templates/page.html`. Local
//! `.md` links are rewritten to `.html` so the docs cross-link as a site.
//!
//! Run from anywhere: `cargo run --manifest-path web/Cargo.toml`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

fn main() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")); // .../web
    let project = base.parent().expect("project root");
    let doc_dir = project.join("doc");
    let content_dir = base.join("content");
    let assets_dir = base.join("assets");
    let template = fs::read_to_string(base.join("templates/page.html"))
        .expect("read templates/page.html");
    let dist = base.join("dist");

    // Fresh output tree.
    let _ = fs::remove_dir_all(&dist);
    fs::create_dir_all(dist.join("docs")).unwrap();

    // Static assets (logo, css) → dist/assets/.
    copy_dir(&assets_dir, &dist.join("assets"));

    // Landing page: content/index.md → dist/index.html (site root).
    let index_md = fs::read_to_string(content_dir.join("index.md")).expect("content/index.md");
    let (title, body) = render(&index_md);
    write_page(&dist.join("index.html"), &template, &title, &body, "", "home");

    // Docs: doc/*.md → dist/docs/<stem>.html.
    let mut docs: Vec<(String, String)> = Vec::new(); // (stem, title)
    let mut entries: Vec<PathBuf> = fs::read_dir(&doc_dir)
        .expect("read doc/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    entries.sort();
    for path in &entries {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let md = fs::read_to_string(path).unwrap();
        let (title, body) = render(&md);
        write_page(
            &dist.join("docs").join(format!("{stem}.html")),
            &template,
            &title,
            &body,
            "../",
            "doc",
        );
        docs.push((stem, title));
    }

    // Docs index: dist/docs/index.html listing every page.
    let mut list = String::from("<h1>Documentation</h1>\n<ul class=\"doc-index\">\n");
    for (stem, title) in &docs {
        list.push_str(&format!("  <li><a href=\"{stem}.html\">{}</a></li>\n", esc(title)));
    }
    list.push_str("</ul>\n");
    write_page(&dist.join("docs/index.html"), &template, "Documentation", &list, "../", "doc");

    println!("Generated {} doc pages + landing page → {}", docs.len(), dist.display());
}

/// Markdown → (title, html_body). Title is the first H1, else "rtdvi".
fn render(md: &str) -> (String, String) {
    let title = md
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|s| strip_inline_md(s.trim()))
        .unwrap_or_else(|| "rtdvi".into());

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let mut events: Vec<Event> = Parser::new_ext(md, opts).map(rewrite_links).collect();
    inject_heading_ids(&mut events);
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    (title, body)
}

/// Give every heading a GitHub-style slug `id` so the `#anchor` links the
/// docs use actually jump. Duplicate slugs get `-1`, `-2`, … suffixes.
fn inject_heading_ids(events: &mut [Event]) {
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut i = 0;
    while i < events.len() {
        if matches!(events[i], Event::Start(Tag::Heading { .. })) {
            // Gather text up to the matching heading end.
            let mut text = String::new();
            let mut j = i + 1;
            while j < events.len() {
                match &events[j] {
                    Event::Text(t) | Event::Code(t) => text.push_str(t),
                    Event::End(TagEnd::Heading(_)) => break,
                    _ => {}
                }
                j += 1;
            }
            let mut slug = slugify(&text);
            let n = seen.entry(slug.clone()).or_insert(0);
            if *n > 0 {
                slug = format!("{slug}-{n}");
            }
            *n += 1;
            if let Event::Start(Tag::Heading { id, .. }) = &mut events[i] {
                if id.is_none() {
                    *id = Some(CowStr::from(slug));
                }
            }
        }
        i += 1;
    }
}

/// GitHub-flavoured heading slug: lowercase, drop punctuation, spaces→`-`.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' || c == '-' || c == '_' {
            out.push('-');
        }
        // everything else (punctuation, backticks already gone) is dropped
    }
    // collapse repeated hyphens, trim ends
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_dash = false;
    for c in out.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push(c);
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    collapsed.trim_matches('-').to_string()
}

/// Rewrite local `*.md` link targets to `*.html` (keeping any `#anchor`),
/// so the generated docs link to each other. External / absolute links and
/// in-page anchors are left untouched.
fn rewrite_links(ev: Event) -> Event {
    if let Event::Start(Tag::Link { link_type, dest_url, title, id }) = ev {
        let new = rewrite_md_target(&dest_url);
        Event::Start(Tag::Link {
            link_type,
            dest_url: CowStr::from(new),
            title,
            id,
        })
    } else {
        ev
    }
}

fn rewrite_md_target(url: &str) -> String {
    if url.contains("://") || url.starts_with('#') || url.starts_with("mailto:") {
        return url.to_string();
    }
    let (path, anchor) = match url.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (url, None),
    };
    let path = if let Some(stem) = path.strip_suffix(".md") {
        format!("{stem}.html")
    } else {
        path.to_string()
    };
    match anchor {
        Some(a) => format!("{path}#{a}"),
        None => path,
    }
}

fn write_page(out: &Path, template: &str, title: &str, body: &str, root: &str, bodyclass: &str) {
    let html = template
        .replace("{{title}}", &esc(title))
        .replace("{{root}}", root)
        .replace("{{bodyclass}}", bodyclass)
        .replace("{{content}}", body)
        .replace("{{year}}", "2026");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(out, html).unwrap();
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    let Ok(entries) = fs::read_dir(from) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest);
        } else {
            fs::copy(&p, &dest).unwrap();
        }
    }
}

/// Very small inline-markdown stripper for titles (code spans, emphasis).
fn strip_inline_md(s: &str) -> String {
    s.replace('`', "").replace("**", "").replace('*', "")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
