pub mod ansi;
pub mod buffer;
pub mod charset;
mod storage;
mod readiness;
pub mod history;
pub mod input;
pub mod pager;
pub mod search;
pub mod terminal;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{parse_keys_from_bytes, Key};
    use crate::pager::LineEditor;

    // =====================================================================
    // Key parsing from raw bytes
    // =====================================================================

    #[test]
    fn parse_ascii_chars() {
        let keys = parse_keys_from_bytes(b"abc");
        assert_eq!(keys, vec![Key::Char('a'), Key::Char('b'), Key::Char('c')]);
    }

    #[test]
    fn parse_enter() {
        let keys = parse_keys_from_bytes(b"\r");
        assert_eq!(keys, vec![Key::Enter]);

        let keys = parse_keys_from_bytes(b"\n");
        assert_eq!(keys, vec![Key::Enter]);
    }

    #[test]
    fn parse_backspace() {
        let keys = parse_keys_from_bytes(b"\x7f");
        assert_eq!(keys, vec![Key::Backspace]);

        let keys = parse_keys_from_bytes(b"\x08");
        assert_eq!(keys, vec![Key::Backspace]);
    }

    #[test]
    fn parse_ctrl_keys() {
        // Ctrl-A = 0x01, Ctrl-C = 0x03, Ctrl-D = 0x04
        let keys = parse_keys_from_bytes(b"\x01");
        assert_eq!(keys, vec![Key::Ctrl('a')]);

        let keys = parse_keys_from_bytes(b"\x03");
        assert_eq!(keys, vec![Key::Ctrl('c')]);

        let keys = parse_keys_from_bytes(b"\x04");
        assert_eq!(keys, vec![Key::Ctrl('d')]);
    }

    #[test]
    fn parse_arrow_keys() {
        let keys = parse_keys_from_bytes(b"\x1b[A");
        assert_eq!(keys, vec![Key::Up]);

        let keys = parse_keys_from_bytes(b"\x1b[B");
        assert_eq!(keys, vec![Key::Down]);

        let keys = parse_keys_from_bytes(b"\x1b[C");
        assert_eq!(keys, vec![Key::Right]);

        let keys = parse_keys_from_bytes(b"\x1b[D");
        assert_eq!(keys, vec![Key::Left]);
    }

    #[test]
    fn parse_home_end() {
        let keys = parse_keys_from_bytes(b"\x1b[H");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[F");
        assert_eq!(keys, vec![Key::End]);
    }

    #[test]
    fn parse_page_up_down() {
        let keys = parse_keys_from_bytes(b"\x1b[5~");
        assert_eq!(keys, vec![Key::PageUp]);

        let keys = parse_keys_from_bytes(b"\x1b[6~");
        assert_eq!(keys, vec![Key::PageDown]);
    }

    #[test]
    fn parse_delete() {
        let keys = parse_keys_from_bytes(b"\x1b[3~");
        assert_eq!(keys, vec![Key::Delete]);
    }

    #[test]
    fn parse_ss3_arrows() {
        let keys = parse_keys_from_bytes(b"\x1bOA");
        assert_eq!(keys, vec![Key::Up]);

        let keys = parse_keys_from_bytes(b"\x1bOB");
        assert_eq!(keys, vec![Key::Down]);
    }

    #[test]
    fn parse_multiple_keys_in_sequence() {
        let keys = parse_keys_from_bytes(b"q\x1b[A\x1b[Bj");
        assert_eq!(
            keys,
            vec![Key::Char('q'), Key::Up, Key::Down, Key::Char('j')]
        );
    }

    #[test]
    fn parse_home_end_numeric_variants() {
        // ESC [ 1 ~ = Home, ESC [ 4 ~ = End
        let keys = parse_keys_from_bytes(b"\x1b[1~");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[4~");
        assert_eq!(keys, vec![Key::End]);

        // ESC [ 7 ~ = Home, ESC [ 8 ~ = End (rxvt style)
        let keys = parse_keys_from_bytes(b"\x1b[7~");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[8~");
        assert_eq!(keys, vec![Key::End]);
    }

    #[test]
    fn parse_modified_arrows_consumed() {
        // ESC [ 1 ; 5 A = Ctrl+Up — should be consumed as Unknown
        let keys = parse_keys_from_bytes(b"\x1b[1;5A");
        assert_eq!(keys, vec![Key::Unknown]);
    }

    #[test]
    fn parse_utf8_char() {
        // é = 0xc3 0xa9
        let keys = parse_keys_from_bytes(&[0xc3, 0xa9]);
        assert_eq!(keys, vec![Key::Char('é')]);

        // 日 = 0xe6 0x97 0xa5
        let keys = parse_keys_from_bytes(&[0xe6, 0x97, 0xa5]);
        assert_eq!(keys, vec![Key::Char('日')]);
    }

    // =====================================================================
    // LineEditor
    // =====================================================================

    #[test]
    fn editor_new_is_empty() {
        let ed = LineEditor::new();
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_insert_ascii() {
        let mut ed = LineEditor::new();
        ed.insert('h');
        ed.insert('i');
        assert_eq!(ed.content, "hi");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_insert_utf8() {
        let mut ed = LineEditor::new();
        ed.insert('é');
        assert_eq!(ed.content, "é");
        assert_eq!(ed.cursor, 2); // é is 2 bytes in UTF-8
    }

    #[test]
    fn editor_backspace() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.backspace();
        assert_eq!(ed.content, "ab");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_backspace_utf8() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('é');
        ed.backspace();
        assert_eq!(ed.content, "a");
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn editor_backspace_at_start() {
        let mut ed = LineEditor::new();
        ed.backspace(); // should be a no-op
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_delete() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.move_home();
        ed.delete();
        assert_eq!(ed.content, "bc");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_delete_at_end() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.delete(); // no-op, cursor at end
        assert_eq!(ed.content, "a");
    }

    #[test]
    fn editor_move_left_right() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        assert_eq!(ed.cursor, 3);

        ed.move_left();
        assert_eq!(ed.cursor, 2);
        ed.move_left();
        assert_eq!(ed.cursor, 1);
        ed.move_right();
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_move_left_at_start() {
        let mut ed = LineEditor::new();
        ed.move_left();
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_move_right_at_end() {
        let mut ed = LineEditor::new();
        ed.insert('x');
        ed.move_right();
        assert_eq!(ed.cursor, 1); // stays at end
    }

    #[test]
    fn editor_home_end() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');

        ed.move_home();
        assert_eq!(ed.cursor, 0);

        ed.move_end();
        assert_eq!(ed.cursor, 3);
    }

    #[test]
    fn editor_clear() {
        let mut ed = LineEditor::new();
        ed.insert('x');
        ed.insert('y');
        ed.clear();
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_insert_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('c');
        ed.move_left();
        ed.insert('b');
        assert_eq!(ed.content, "abc");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_backspace_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.move_left(); // cursor after 'b'
        ed.backspace();
        assert_eq!(ed.content, "ac");
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn editor_delete_utf8_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('é');
        ed.insert('b');
        ed.move_home();
        ed.move_right(); // after 'a', before 'é'
        ed.delete();
        assert_eq!(ed.content, "ab");
    }

    #[test]
    fn source_complete_and_unterminated_lines() {
        let mut lb = buffer::LineBuffer::from_lines(vec!["line1".into(), "line2".into(), "line3".into()]);
        assert_eq!(lb.get_line(0), Some("line1"));
        assert_eq!(lb.get_line(1), Some("line2"));
        assert_eq!(lb.get_line(2), Some("line3"));
        assert_eq!(lb.get_line(3), None);
    }
}
