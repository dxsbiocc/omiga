"""
Tests for nanoclaw/router.py
"""
from omiga.models import NewMessage
from omiga.router import (
    escape_xml,
    format_messages,
    format_outbound,
    parse_file_directives,
    strip_internal_tags,
)


def test_escape_xml_ampersand():
    assert escape_xml("a&b") == "a&amp;b"


def test_escape_xml_lt_gt():
    assert escape_xml("<tag>") == "&lt;tag&gt;"


def test_escape_xml_quote():
    assert escape_xml('"hello"') == "&quot;hello&quot;"


def test_escape_xml_empty():
    assert escape_xml("") == ""


def test_format_messages_basic():
    msgs = [
        NewMessage(
            id="1", chat_jid="jid", sender="s", sender_name="Alice",
            content="Hi", timestamp="2024-01-01T00:00:00Z",
        )
    ]
    xml = format_messages(msgs)
    assert xml.startswith("<messages>")
    assert xml.endswith("</messages>")
    assert 'sender="Alice"' in xml
    assert "Hi" in xml


def test_format_messages_escapes_content():
    msgs = [
        NewMessage(
            id="1", chat_jid="jid", sender="s", sender_name="Bob",
            content="a < b & c", timestamp="2024-01-01T00:00:00Z",
        )
    ]
    xml = format_messages(msgs)
    assert "&lt;" in xml
    assert "&amp;" in xml


def test_strip_internal_tags():
    text = "Hello <internal>secret reasoning</internal> world"
    assert strip_internal_tags(text) == "Hello  world"


def test_strip_internal_tags_multiline():
    text = "prefix <internal>\nmulti\nline\n</internal> suffix"
    assert strip_internal_tags(text) == "prefix  suffix"


def test_strip_internal_tags_no_tags():
    text = "plain text"
    assert strip_internal_tags(text) == "plain text"


def test_format_outbound_strips_internal():
    raw = "Result <internal>thinking</internal>"
    assert format_outbound(raw) == "Result"


def test_format_outbound_empty_after_strip():
    raw = "<internal>only internal</internal>"
    assert format_outbound(raw) == ""


def test_format_outbound_plain():
    assert format_outbound("hello world") == "hello world"


# ---------------------------------------------------------------------------
# parse_file_directives
# ---------------------------------------------------------------------------

def test_parse_file_directives_no_directive():
    text = "Hello, here is your answer."
    clean, files = parse_file_directives(text)
    assert clean == text
    assert files == []


def test_parse_file_directives_single():
    text = "Here is the chart.\n[SEND_FILE: output/chart.png]"
    clean, files = parse_file_directives(text)
    assert clean == "Here is the chart."
    assert len(files) == 1
    assert files[0].workspace_rel_path == "output/chart.png"
    assert files[0].caption == ""


def test_parse_file_directives_with_caption():
    text = "[SEND_FILE: output/report.pdf | Monthly report]"
    clean, files = parse_file_directives(text)
    assert clean == ""
    assert files[0].caption == "Monthly report"


def test_parse_file_directives_multiple():
    text = "Files:\n[SEND_FILE: a.png | First]\n[SEND_FILE: b.pdf | Second]"
    clean, files = parse_file_directives(text)
    assert "Files:" in clean
    assert len(files) == 2
    assert files[0].workspace_rel_path == "a.png"
    assert files[1].workspace_rel_path == "b.pdf"


def test_parse_file_directives_case_insensitive():
    text = "[send_file: output/chart.png]"
    _, files = parse_file_directives(text)
    assert len(files) == 1


def test_parse_file_directives_strips_whitespace_in_path():
    text = "[SEND_FILE:   output/photo.jpg   ]"
    _, files = parse_file_directives(text)
    assert files[0].workspace_rel_path == "output/photo.jpg"


def test_parse_file_directives_inline_mixed():
    text = "See [SEND_FILE: chart.png] and [SEND_FILE: data.csv | Raw data] for details."
    clean, files = parse_file_directives(text)
    assert "See" in clean
    assert "for details." in clean
    assert len(files) == 2
