#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewportState {
    pub(super) scroll_top: usize,
    content_height: usize,
    pub(super) viewport_height: usize,
    pub(super) follow_tail: bool,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            scroll_top: 0,
            content_height: 0,
            viewport_height: 0,
            follow_tail: true,
        }
    }
}

impl ViewportState {
    pub(super) fn max_scroll(&self) -> usize {
        self.content_height.saturating_sub(self.viewport_height)
    }

    pub(super) fn set_content_height(&mut self, height: usize) {
        self.content_height = height;
        self.clamp_scroll();
    }

    pub(super) fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
        self.clamp_scroll();
    }

    pub(super) fn scroll_up(&mut self, lines: usize) {
        let current = if self.follow_tail {
            self.max_scroll()
        } else {
            self.scroll_top
        };
        self.follow_tail = false;
        self.scroll_top = current.saturating_sub(lines);
    }

    pub(super) fn scroll_down(&mut self, lines: usize) {
        self.scroll_top = self.scroll_top.saturating_add(lines).min(self.max_scroll());
        self.follow_tail = self.scroll_top == self.max_scroll();
    }

    pub(super) fn jump_to_bottom(&mut self) {
        self.follow_tail = true;
        self.scroll_top = self.max_scroll();
    }

    pub(super) fn on_new_output(&mut self) {
        self.clamp_scroll();
    }

    pub(super) fn on_submit_prompt(&mut self) {
        self.jump_to_bottom();
    }

    fn clamp_scroll(&mut self) {
        let max_scroll = self.max_scroll();
        if self.follow_tail {
            self.scroll_top = max_scroll;
        } else {
            self.scroll_top = self.scroll_top.min(max_scroll);
        }
    }
}
