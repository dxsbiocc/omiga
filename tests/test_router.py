"""
Tests for nanoclaw/router.py
"""
from omiga.models import NewMessage
from omiga.router import (
    escape_xml,
    format_messages,
    format_outbound,
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
