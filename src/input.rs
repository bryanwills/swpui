#[derive(Debug, Default)]
pub struct TextInput {
    content: String,
    cursor: usize,
    changed: bool,
}

impl TextInput {
    pub fn value(&self) -> &str {
        &self.content
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, c: char) {
        let byte_idx = self.byte_index();
        self.content.insert(byte_idx, c);
        self.cursor += 1;
        self.changed = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_idx = self.byte_index();
            let next_byte_idx = self.content[byte_idx..]
                .char_indices()
                .nth(1)
                .map_or(self.content.len(), |(i, _)| byte_idx + i);
            self.content.drain(byte_idx..next_byte_idx);
            self.changed = true;
        }
    }

    pub fn delete(&mut self) {
        let byte_idx = self.byte_index();
        if byte_idx < self.content.len() {
            let next_byte_idx = self.content[byte_idx..]
                .char_indices()
                .nth(1)
                .map_or(self.content.len(), |(i, _)| byte_idx + i);
            self.content.drain(byte_idx..next_byte_idx);
            self.changed = true;
        }
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.char_count() {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
        self.changed = true;
    }

    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn byte_index(&self) -> usize {
        self.content
            .char_indices()
            .nth(self.cursor)
            .map_or(self.content.len(), |(i, _)| i)
    }

    fn char_count(&self) -> usize {
        self.content.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_is_empty() {
        let input = TextInput::default();
        assert_eq!(input.value(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn insert_char() {
        let mut input = TextInput::default();
        input.insert('h');
        input.insert('i');
        assert_eq!(input.value(), "hi");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_deletes_before_cursor() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        input.backspace();
        assert_eq!(input.value(), "ab");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut input = TextInput::default();
        input.backspace();
        assert_eq!(input.value(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn delete_removes_after_cursor() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        input.move_home();
        input.delete();
        assert_eq!(input.value(), "bc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn delete_at_end_does_nothing() {
        let mut input = TextInput::default();
        input.insert('a');
        input.delete();
        assert_eq!(input.value(), "a");
    }

    #[test]
    fn move_left_right() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        input.move_left();
        assert_eq!(input.cursor(), 1);
        input.move_right();
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn move_left_at_start_stays() {
        let mut input = TextInput::default();
        input.move_left();
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn move_right_at_end_stays() {
        let mut input = TextInput::default();
        input.insert('a');
        input.move_right();
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn home_and_end() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        input.move_home();
        assert_eq!(input.cursor(), 0);
        input.move_end();
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn insert_in_middle() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('c');
        input.move_left();
        input.insert('b');
        assert_eq!(input.value(), "abc");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn unicode_handling() {
        let mut input = TextInput::default();
        input.insert('é');
        input.insert('ñ');
        assert_eq!(input.value(), "éñ");
        assert_eq!(input.cursor(), 2);
        input.backspace();
        assert_eq!(input.value(), "é");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn clear() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        input.clear();
        assert_eq!(input.value(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn changed_flag() {
        let mut input = TextInput::default();
        assert!(!input.take_changed());
        input.insert('a');
        assert!(input.take_changed());
        assert!(!input.take_changed());
    }
}
