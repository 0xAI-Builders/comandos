#!/usr/bin/env python3
import json
import re
import subprocess
import textwrap
from pathlib import Path


HTML = Path("dash/term.html").read_text()


def extract_js_function(source, name):
    needle = f"function {name}("
    start = source.index(needle)
    brace = source.index("{", start)
    depth = 0
    for index in range(brace, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise AssertionError(f"Could not extract {name}")


def test_web_link_handler_clicks_temporary_no_referrer_anchor():
    handler = extract_js_function(HTML, "openWebLink")
    script = textwrap.dedent(
        f"""
        {handler}
        const url = "https://example.com/path?q=brave#terminal";
        const events = [];
        const anchor = {{
          click() {{
            events.push({{
              type: "click",
              href: this.href,
              target: this.target,
              rel: this.rel,
              referrerPolicy: this.referrerPolicy,
            }});
          }},
          remove() {{ events.push({{ type: "remove" }}); }},
        }};
        const document = {{
          createElement(tag) {{
            if (tag !== "a") throw new Error(`unexpected element: ${{tag}}`);
            return anchor;
          }},
          body: {{
            appendChild(element) {{
              if (element !== anchor) throw new Error("unexpected anchor");
              events.push({{ type: "append" }});
            }},
          }},
        }};

        openWebLink({{ type: "click" }}, url);
        console.log(JSON.stringify(events));
        """
    )

    result = json.loads(
        subprocess.check_output(["node", "-e", script], text=True)
    )

    assert result == [
        {"type": "append"},
        {
            "type": "click",
            "href": "https://example.com/path?q=brave#terminal",
            "target": "_blank",
            "rel": "noopener noreferrer",
            "referrerPolicy": "no-referrer",
        },
        {"type": "remove"},
    ]


def test_web_links_addon_uses_named_handler():
    assert re.search(
        r"new\s+WebLinksAddon\.WebLinksAddon\(\s*openWebLink\s*\)", HTML
    )
    assert "new WebLinksAddon.WebLinksAddon()" not in HTML


def test_web_link_handler_has_no_popup_or_frame_navigation_fallback():
    handler = extract_js_function(HTML, "openWebLink")

    assert not re.search(
        r"\bwindow\s*(?:\.\s*open|\[\s*['\"]open['\"]\s*\])\s*\(", handler
    )
    assert not re.search(r"\blocation\b", handler)
    assert not re.search(r"\b(?:window\s*\.\s*)?navigate\s*\(", handler)
