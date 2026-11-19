//! The tools panel's state and work: notes, the pomodoro timer, the
//! calculator, this session's clipboard, snippets and bookmarks, and the
//! system's figures.
//!
//! `tools.json` beside `layout.json` keeps what outlives a start. The panel
//! shows what this puts in the `Tools` global; everything the panel asks for
//! comes back as a callback. The clipboard and the system figures are read
//! only while the panel, or the tab that shows them, is open.

use std::{collections::VecDeque, time::Duration};

use slint::{ModelRc, TimerMode, VecModel};

use super::*;

/// The file beside `layout.json` that keeps the notes, snippets, bookmarks
/// and finished sessions.
const TOOLS: &str = "tools.json";
/// A pomodoro's focus and rest, in seconds.
const FOCUS: u32 = 25 * 60;
const BREAK: u32 = 5 * 60;
/// The clips this session keeps: enough for what was copied lately, not
/// enough to hold a day's worth of text in memory.
const CLIPS: usize = 40;
/// The snippets and bookmarks a file keeps, so one can't grow without end.
const SNIPPETS: usize = 200;
const BOOKMARKS: usize = 300;
/// How many bytes a clip keeps. Pictures and files copied as paths can be
/// long; a message is rarely near this.
const CLIP_BYTES: usize = 8 * 1024;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Kept {
    notes: String,
    snippets: Vec<KeptSnippet>,
    bookmarks: Vec<KeptBookmark>,
    /// Focus sessions finished, across starts.
    sessions: u32,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct KeptSnippet {
    name: String,
    body: String,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct KeptBookmark {
    room: String,
    room_name: String,
    item: String,
    author: String,
    body: String,
    when: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Focus,
    Break,
}

/// The panel's state for as long as Litecord runs.
pub(super) struct Panel {
    kept: Kept,
    /// Notes changed since the last write.
    dirty: bool,
    /// The tab the panel was last on, kept across openings.
    tab: i32,
    clips: VecDeque<ToolClip>,
    /// The clipboard's text as last taken, so the same text isn't kept twice.
    clipboard_seen: Option<String>,
    clipboard: Option<arboard::Clipboard>,
    phase: Phase,
    /// The seconds left in the phase.
    remaining: u32,
    running: bool,
    calc_input: String,
    calc_result: String,
    /// The save, the second, and the two readers.
    save: slint::Timer,
    tick: slint::Timer,
    clip_timer: slint::Timer,
    system_timer: slint::Timer,
    flash: slint::Timer,
}

impl Panel {
    pub(super) fn new(root: &std::path::Path) -> Self {
        let kept: Kept = std::fs::read(root.join(TOOLS))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            kept,
            dirty: false,
            tab: 0,
            clips: VecDeque::new(),
            clipboard_seen: None,
            clipboard: None,
            phase: Phase::Focus,
            remaining: FOCUS,
            running: false,
            calc_input: String::new(),
            calc_result: "0".into(),
            save: slint::Timer::default(),
            tick: slint::Timer::default(),
            clip_timer: slint::Timer::default(),
            system_timer: slint::Timer::default(),
            flash: slint::Timer::default(),
        }
    }

    /// Writes what matters if the last edit didn't get its chance.
    pub(super) fn flush(&self, root: &std::path::Path) {
        if !self.dirty {
            return;
        }
        let result = std::fs::create_dir_all(root).and_then(|()| {
            std::fs::write(
                root.join(TOOLS),
                serde_json::to_vec(&self.kept).expect("the tools' file serializes"),
            )
        });
        if let Err(error) = result {
            tracing::warn!(%error, "could not store the tools");
        }
    }

    fn clock(&self) -> String {
        let remaining = self.remaining;
        format!("{}:{:02}", remaining / 60, remaining % 60)
    }

    fn progress(&self) -> f32 {
        let full = match self.phase {
            Phase::Focus => FOCUS,
            Phase::Break => BREAK,
        };
        if full == 0 {
            0.0
        } else {
            1.0 - self.remaining as f32 / full as f32
        }
    }
}

impl Adapter {
    /// Opens the panel, or closes it if it is open.
    pub(super) fn toggle_tools(&mut self, window: &LitecordWindow) {
        if window.global::<Tools>().get_open() {
            self.close_tools(window);
        } else {
            self.open_tools(window);
        }
    }

    pub(super) fn open_tools(&mut self, window: &LitecordWindow) {
        // The panel is over everything, so nothing that asks for the same
        // place stays open beneath it.
        window.set_settings_open(false);
        window.set_server_settings_open(false);
        window.set_pins_open(false);
        window.set_threads_open(false);
        self.sync_tools(window);
        let tools = window.global::<Tools>();
        tools.set_open(true);
        tools.set_status("".into());
        tools.set_notes_note("Saved on this computer.".into());
        tools.set_snippet_name("".into());
        tools.set_snippet_body("".into());
        tools.set_data_folder(self.root.display().to_string().into());
        tools.set_build(crate::build_info::current().summary().into());
        self.sync_tools_timer(window);
        self.refresh_system(window);
        self.restart_tools_readers(window);
    }

    pub(super) fn close_tools(&mut self, window: &LitecordWindow) {
        window.global::<Tools>().set_open(false);
        self.tools.clip_timer.stop();
        self.tools.system_timer.stop();
        self.tools.clipboard = None;
        self.tools.clipboard_seen = None;
        if !self.tools.running {
            self.tools.tick.stop();
        }
        self.save_tools();
    }

    /// Puts the kept lists and the notes in the panel. Called opening, and
    /// after anything changes a list.
    fn sync_tools(&self, window: &LitecordWindow) {
        let tools = window.global::<Tools>();
        tools.set_notes(self.tools.kept.notes.as_str().into());
        tools.set_tab(self.tools.tab);
        tools.set_clips(ModelRc::new(VecModel::from(
            self.tools.clips.iter().cloned().collect::<Vec<_>>(),
        )));
        let snippets: Vec<ToolSnippet> = self
            .tools
            .kept
            .snippets
            .iter()
            .map(|snippet| ToolSnippet {
                name: snippet.name.as_str().into(),
                body: snippet.body.as_str().into(),
            })
            .collect();
        tools.set_snippets(ModelRc::new(VecModel::from(snippets)));
        let bookmarks: Vec<ToolBookmark> = self
            .tools
            .kept
            .bookmarks
            .iter()
            .map(|bookmark| ToolBookmark {
                room: bookmark.room.as_str().into(),
                room_name: bookmark.room_name.as_str().into(),
                item: bookmark.item.as_str().into(),
                author: bookmark.author.as_str().into(),
                body: bookmark.body.as_str().into(),
                when: bookmark.when.as_str().into(),
            })
            .collect();
        tools.set_bookmarks(ModelRc::new(VecModel::from(bookmarks)));
        tools.set_calc_input(self.tools.calc_input.as_str().into());
        tools.set_calc_result(self.tools.calc_result.as_str().into());
        tools.set_calc_note(self.tools.calc_note().into());
    }

    fn sync_tools_timer(&self, window: &LitecordWindow) {
        let tools = window.global::<Tools>();
        tools.set_timer_phase(
            match self.tools.phase {
                Phase::Focus => "Focus",
                Phase::Break => "Break",
            }
            .into(),
        );
        tools.set_timer_clock(self.tools.clock().into());
        tools.set_timer_progress(self.tools.progress());
        tools.set_timer_running(self.tools.running);
        tools.set_timer_sessions(self.tools.kept.sessions as i32);
        tools.set_timer_note(
            match self.tools.phase {
                Phase::Focus => "25 minutes of focus, then 5 minutes away.",
                Phase::Break => "Five minutes away. The next focus starts when this ends.",
            }
            .into(),
        );
    }

    fn restart_tools_readers(&mut self, window: &LitecordWindow) {
        self.tools.clip_timer.stop();
        self.tools.system_timer.stop();
        let tools = window.global::<Tools>();
        if !tools.get_open() {
            return;
        }
        match tools.get_tab() {
            3 => self.start_clipboard_reader(window),
            5 => self.start_system_reader(window),
            _ => {}
        }
    }

    fn start_clipboard_reader(&mut self, window: &LitecordWindow) {
        let weak = self.weak.clone();
        let win = window.as_weak();
        self.tools
            .clip_timer
            .start(TimerMode::Repeated, Duration::from_millis(900), move || {
                let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
                    return;
                };
                adapter.borrow_mut().poll_clipboard(&window);
            });
        // The first look comes now, not in a second: the panel opens showing
        // what is on the clipboard already.
        self.poll_clipboard(window);
    }

    fn start_system_reader(&mut self, window: &LitecordWindow) {
        let weak = self.weak.clone();
        let win = window.as_weak();
        self.tools
            .system_timer
            .start(TimerMode::Repeated, Duration::from_secs(3), move || {
                let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
                    return;
                };
                adapter.borrow_mut().refresh_system(&window);
            });
    }

    pub(super) fn tools_tab_changed(&mut self, window: &LitecordWindow, tab: i32) {
        self.tools.tab = tab;
        window.global::<Tools>().set_status(Default::default());
        self.tools.clip_timer.stop();
        self.tools.system_timer.stop();
        if !window.global::<Tools>().get_open() {
            return;
        }
        match tab {
            3 => self.start_clipboard_reader(window),
            5 => {
                self.refresh_system(window);
                self.start_system_reader(window);
            }
            _ => {
                self.tools.clipboard = None;
                self.tools.clipboard_seen = None;
            }
        }
    }

    /// Takes what the clipboard holds if it changed since the last look.
    fn poll_clipboard(&mut self, window: &LitecordWindow) {
        let tools = window.global::<Tools>();
        if !tools.get_open() || tools.get_tab() != 3 {
            return;
        }
        if self.tools.clipboard.is_none() {
            self.tools.clipboard = arboard::Clipboard::new().ok();
        }
        let Some(clipboard) = self.tools.clipboard.as_mut() else {
            return;
        };
        let Ok(text) = clipboard.get_text() else {
            return;
        };
        if text.trim().is_empty() || self.tools.clipboard_seen.as_deref() == Some(text.as_str()) {
            return;
        }
        let text = if text.len() > CLIP_BYTES {
            let mut end = CLIP_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_owned()
        } else {
            text
        };
        self.tools.clipboard_seen = Some(text.clone());
        self.tools
            .clips
            .retain(|clip| clip.text.as_str() != text.as_str());
        self.tools.clips.push_front(ToolClip {
            text: text.as_str().into(),
            when: clock::time(clock::now_ms() / 1000).into(),
        });
        self.tools.clips.truncate(CLIPS);
        self.show_clips(window);
    }

    fn show_clips(&self, window: &LitecordWindow) {
        window
            .global::<Tools>()
            .set_clips(ModelRc::new(VecModel::from(
                self.tools.clips.iter().cloned().collect::<Vec<_>>(),
            )));
    }

    pub(super) fn clear_clips(&mut self, window: &LitecordWindow) {
        self.tools.clips.clear();
        self.show_clips(window);
        self.flash_tools(window, "Clipboard history cleared.");
    }

    pub(super) fn tools_notes_edited(&mut self, window: &LitecordWindow, notes: String) {
        self.tools.kept.notes = notes;
        self.tools.dirty = true;
        window.global::<Tools>().set_notes_note("Saving...".into());
        self.schedule_tools_save();
    }

    fn schedule_tools_save(&self) {
        let weak = self.weak.clone();
        self.tools.save.start(
            TimerMode::SingleShot,
            Duration::from_millis(700),
            move || {
                let Some(adapter) = weak.upgrade() else {
                    return;
                };
                let mut adapter = adapter.borrow_mut();
                adapter.save_tools();
                if let Some(window) = adapter.window.upgrade() {
                    window.global::<Tools>().set_notes_note("Saved.".into());
                }
            },
        );
    }

    fn save_tools(&mut self) {
        if !self.tools.dirty {
            return;
        }
        self.tools.flush(&self.root);
        self.tools.dirty = false;
    }

    fn start_tools_tick(&mut self, window: &LitecordWindow) {
        let weak = self.weak.clone();
        let win = window.as_weak();
        self.tools
            .tick
            .start(TimerMode::Repeated, Duration::from_secs(1), move || {
                let (Some(adapter), Some(window)) = (weak.upgrade(), win.upgrade()) else {
                    return;
                };
                adapter.borrow_mut().tools_tick(&window);
            });
    }

    fn tools_tick(&mut self, window: &LitecordWindow) {
        if !self.tools.running {
            self.tools.tick.stop();
            return;
        }
        if self.tools.remaining > 0 {
            self.tools.remaining -= 1;
        }
        if self.tools.remaining == 0 {
            self.finish_timer_phase(window);
        }
        if window.global::<Tools>().get_open() {
            self.sync_tools_timer(window);
        }
        if !self.tools.running {
            self.tools.tick.stop();
        }
    }

    fn finish_timer_phase(&mut self, window: &LitecordWindow) {
        match self.tools.phase {
            Phase::Focus => {
                self.tools.kept.sessions = self.tools.kept.sessions.saturating_add(1);
                self.tools.phase = Phase::Break;
                self.tools.remaining = BREAK;
                // A break follows a finished focus on its own, so the person
                // can step away without coming back to a stopped clock.
                self.play(Sound::Message);
                self.save_kept();
            }
            Phase::Break => {
                self.tools.phase = Phase::Focus;
                self.tools.remaining = FOCUS;
                self.tools.running = false;
                self.play(Sound::Message);
            }
        }
        if window.global::<Tools>().get_open() {
            self.sync_tools_timer(window);
            self.flash_tools(
                window,
                if self.tools.phase == Phase::Break {
                    "Focus finished. Take five."
                } else {
                    "Break finished. Ready for the next focus."
                },
            );
        }
    }

    pub(super) fn tools_timer_start(&mut self, window: &LitecordWindow) {
        if self.tools.remaining == 0 {
            self.tools.remaining = match self.tools.phase {
                Phase::Focus => FOCUS,
                Phase::Break => BREAK,
            };
        }
        self.tools.running = true;
        self.start_tools_tick(window);
        self.sync_tools_timer(window);
    }

    pub(super) fn tools_timer_pause(&mut self, window: &LitecordWindow) {
        self.tools.running = false;
        self.tools.tick.stop();
        self.sync_tools_timer(window);
    }

    pub(super) fn tools_timer_reset(&mut self, window: &LitecordWindow) {
        self.tools.running = false;
        self.tools.tick.stop();
        self.tools.remaining = match self.tools.phase {
            Phase::Focus => FOCUS,
            Phase::Break => BREAK,
        };
        self.sync_tools_timer(window);
    }

    pub(super) fn tools_timer_skip(&mut self, window: &LitecordWindow) {
        let was_running = self.tools.running;
        self.tools.phase = match self.tools.phase {
            Phase::Focus => Phase::Break,
            Phase::Break => Phase::Focus,
        };
        self.tools.remaining = match self.tools.phase {
            Phase::Focus => FOCUS,
            Phase::Break => BREAK,
        };
        self.sync_tools_timer(window);
        if was_running {
            self.start_tools_tick(window);
        }
    }

    pub(super) fn tools_calc_edited(&mut self, window: &LitecordWindow, input: String) {
        self.tools.calc_input = input;
        self.evaluate_calc(window);
    }

    pub(super) fn tools_calc_key(&mut self, window: &LitecordWindow, key: String) {
        match key.as_str() {
            "C" => self.tools.calc_input.clear(),
            "back" => {
                self.tools.calc_input.pop();
            }
            "=" => {
                if self.tools.calc_result != "-"
                    && (!self.tools.calc_result.is_empty())
                    && calculate(self.tools.calc_input.trim()).is_ok()
                {
                    self.tools.calc_input = self.tools.calc_result.clone();
                }
            }
            _ => {
                if self.tools.calc_input.len() + key.len() <= 400 {
                    self.tools.calc_input.push_str(&key);
                }
            }
        }
        window
            .global::<Tools>()
            .set_calc_input(self.tools.calc_input.as_str().into());
        self.evaluate_calc(window);
    }

    fn evaluate_calc(&mut self, window: &LitecordWindow) {
        let expression = self.tools.calc_input.trim();
        if expression.is_empty() {
            self.tools.calc_result = "0".into();
            let tools = window.global::<Tools>();
            tools.set_calc_result("0".into());
            tools.set_calc_note("Type an expression, or use the keys.".into());
            return;
        }
        let tools = window.global::<Tools>();
        match calculate(expression) {
            Ok(value) => {
                self.tools.calc_result = format_number(value);
                tools.set_calc_result(self.tools.calc_result.as_str().into());
                tools.set_calc_note("Press = to keep the result in the box.".into());
            }
            Err(note) => {
                self.tools.calc_result = "-".into();
                tools.set_calc_result("-".into());
                tools.set_calc_note(note.into());
            }
        }
    }

    pub(super) fn tools_copy(&mut self, window: &LitecordWindow, text: String) {
        if text.is_empty() {
            return;
        }
        window.invoke_copy_text(text.into());
        self.flash_tools(window, "Copied.");
    }

    /// Puts text in the open conversation's composer and closes the panel.
    pub(super) fn tools_insert(&mut self, window: &LitecordWindow, text: String) {
        if self.app.state.selected.is_none() {
            self.flash_tools(window, "Open a channel to put this in its composer.");
            return;
        }
        if text.is_empty() {
            return;
        }
        let caret = i32::try_from(text.len()).unwrap_or(i32::MAX);
        self.close_tools(window);
        self.action(window, Action::SetDraft(text));
        window.invoke_place_composer_caret(caret);
        window.invoke_focus_composer();
    }

    pub(super) fn tools_add_snippet(&mut self, window: &LitecordWindow) {
        let tools = window.global::<Tools>();
        let name = tools.get_snippet_name().trim().to_owned();
        let body = tools.get_snippet_body().trim().to_owned();
        if body.is_empty() {
            self.flash_tools(window, "Write the snippet's text first.");
            return;
        }
        self.tools
            .kept
            .snippets
            .insert(0, KeptSnippet { name, body });
        self.tools.kept.snippets.truncate(SNIPPETS);
        self.show_snippets(window);
        tools.set_snippet_name("".into());
        tools.set_snippet_body("".into());
        self.save_kept();
        self.flash_tools(window, "Snippet added.");
    }

    fn show_snippets(&self, window: &LitecordWindow) {
        let snippets: Vec<ToolSnippet> = self
            .tools
            .kept
            .snippets
            .iter()
            .map(|snippet| ToolSnippet {
                name: snippet.name.as_str().into(),
                body: snippet.body.as_str().into(),
            })
            .collect();
        window
            .global::<Tools>()
            .set_snippets(ModelRc::new(VecModel::from(snippets)));
    }

    pub(super) fn tools_use_snippet(&mut self, window: &LitecordWindow, index: i32) {
        let Some(snippet) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tools.kept.snippets.get(index))
        else {
            return;
        };
        self.tools_insert(window, snippet.body.clone());
    }

    pub(super) fn tools_copy_snippet(&mut self, window: &LitecordWindow, index: i32) {
        let Some(snippet) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tools.kept.snippets.get(index))
        else {
            return;
        };
        self.tools_copy(window, snippet.body.clone());
    }

    pub(super) fn tools_delete_snippet(&mut self, window: &LitecordWindow, index: i32) {
        let Some(index) = usize::try_from(index).ok() else {
            return;
        };
        if index >= self.tools.kept.snippets.len() {
            return;
        }
        self.tools.kept.snippets.remove(index);
        self.show_snippets(window);
        self.save_kept();
        self.flash_tools(window, "Snippet removed.");
    }

    /// Keeps a message from its menu, newest first, once.
    pub(super) fn bookmark_message(
        &mut self,
        window: &LitecordWindow,
        room: String,
        room_name: String,
        item: String,
        author: String,
        body: String,
        when: String,
    ) {
        if item.is_empty() || body.is_empty() {
            return;
        }
        self.tools
            .kept
            .bookmarks
            .retain(|bookmark| !(bookmark.item == item && bookmark.room == room));
        self.tools.kept.bookmarks.insert(
            0,
            KeptBookmark {
                room,
                room_name,
                item,
                author,
                body,
                when,
            },
        );
        self.tools.kept.bookmarks.truncate(BOOKMARKS);
        if window.global::<Tools>().get_open() {
            self.show_bookmarks(window);
            self.flash_tools(window, "Saved to Tools > Saved.");
        }
        self.save_kept();
    }

    fn show_bookmarks(&self, window: &LitecordWindow) {
        let bookmarks: Vec<ToolBookmark> = self
            .tools
            .kept
            .bookmarks
            .iter()
            .map(|bookmark| ToolBookmark {
                room: bookmark.room.as_str().into(),
                room_name: bookmark.room_name.as_str().into(),
                item: bookmark.item.as_str().into(),
                author: bookmark.author.as_str().into(),
                body: bookmark.body.as_str().into(),
                when: bookmark.when.as_str().into(),
            })
            .collect();
        window
            .global::<Tools>()
            .set_bookmarks(ModelRc::new(VecModel::from(bookmarks)));
    }

    pub(super) fn tools_jump_bookmark(&mut self, window: &LitecordWindow, index: i32) {
        let Some(bookmark) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tools.kept.bookmarks.get(index))
        else {
            return;
        };
        let (room, item) = (bookmark.room.clone(), bookmark.item.clone());
        self.close_tools(window);
        self.action(window, Action::JumpInRoom { room, item });
    }

    pub(super) fn tools_copy_bookmark(&mut self, window: &LitecordWindow, index: i32) {
        let Some(bookmark) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tools.kept.bookmarks.get(index))
        else {
            return;
        };
        self.tools_copy(window, bookmark.body.clone());
    }

    pub(super) fn tools_delete_bookmark(&mut self, window: &LitecordWindow, index: i32) {
        let Some(index) = usize::try_from(index).ok() else {
            return;
        };
        if index >= self.tools.kept.bookmarks.len() {
            return;
        }
        self.tools.kept.bookmarks.remove(index);
        self.show_bookmarks(window);
        self.save_kept();
        self.flash_tools(window, "Bookmark removed.");
    }

    fn save_kept(&mut self) {
        self.tools.dirty = true;
        self.save_tools();
    }

    /// What Litecord takes, the computer's memory and what the media caches
    /// hold, for the System tab.
    pub(super) fn refresh_system(&mut self, window: &LitecordWindow) {
        let tools = window.global::<Tools>();
        tools.set_own_memory(own_memory().into());
        tools.set_system_memory(system_memory().into());
        tools.set_cache_size(cache_size().into());
    }

    pub(super) fn clear_media_cache(&mut self, window: &LitecordWindow) {
        #[cfg(feature = "discord")]
        let freed = litecord_core::litecord_discord::cache::clear_media();
        #[cfg(not(feature = "discord"))]
        let freed: u64 = 0;
        self.refresh_system(window);
        self.flash_tools(
            window,
            if freed == 0 {
                "Nothing in the media cache was freed."
            } else {
                "Media cache cleared. Pictures come back as they are shown."
            },
        );
    }

    pub(super) fn open_data_folder(&mut self, window: &LitecordWindow) {
        let result = open_folder(&self.root);
        if let Err(error) = result {
            tracing::warn!(%error, "could not open the data folder");
            self.flash_tools(window, "Could not open the data folder.");
        }
    }

    /// A short note under the panel, gone after a moment.
    fn flash_tools(&mut self, window: &LitecordWindow, text: impl Into<SharedString>) {
        let tools = window.global::<Tools>();
        tools.set_status(text.into());
        let weak = window.as_weak();
        self.tools.flash.start(
            TimerMode::SingleShot,
            Duration::from_millis(3500),
            move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<Tools>().set_status("".into());
                }
            },
        );
    }
}

/// What the tools panel's buttons and fields ask for.
pub(super) fn bind(window: &LitecordWindow, adapter: &Rc<RefCell<Adapter>>) {
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_close(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().close_tools(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_tab_changed(move |tab| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_tab_changed(&window, tab);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_notes_edited(move |notes| {
        let Some(window) = weak.upgrade() else { return };
        state
            .borrow_mut()
            .tools_notes_edited(&window, notes.to_string());
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_timer_start(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_timer_start(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_timer_pause(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_timer_pause(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_timer_reset(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_timer_reset(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_timer_skip(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_timer_skip(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_calc_edited(move |input| {
        let Some(window) = weak.upgrade() else { return };
        state
            .borrow_mut()
            .tools_calc_edited(&window, input.to_string());
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_calc_key(move |key| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_calc_key(&window, key.to_string());
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_copy(move |text| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_copy(&window, text.to_string());
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_insert(move |text| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_insert(&window, text.to_string());
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_clear_clips(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().clear_clips(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_add_snippet(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_add_snippet(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_use_snippet(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_use_snippet(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_copy_snippet(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_copy_snippet(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_delete_snippet(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_delete_snippet(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_jump_bookmark(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_jump_bookmark(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_copy_bookmark(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_copy_bookmark(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_delete_bookmark(move |index| {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().tools_delete_bookmark(&window, index);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_clear_cache(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().clear_media_cache(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_open_data_folder(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().open_data_folder(&window);
    });
    let state = adapter.clone();
    let weak = window.as_weak();
    window.global::<Tools>().on_refresh_system(move || {
        let Some(window) = weak.upgrade() else { return };
        state.borrow_mut().refresh_system(&window);
    });
}

impl Panel {
    fn calc_note(&self) -> &'static str {
        if self.calc_input.trim().is_empty() {
            "Type an expression, or use the keys."
        } else {
            "Press = to keep the result in the box."
        }
    }
}

/// What the System tab says Litecord itself takes: its working set and its
/// committed private bytes.
fn own_memory() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        // GetProcessMemoryInfo takes the shorter structure's pointer; the
        // longer one starts with it and adds the private bytes.
        let worked = unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                counters.cb,
            )
        } != 0;
        if worked {
            return format!(
                "{} working set, {} private",
                bytes(counters.WorkingSetSize as u64),
                bytes(counters.PrivateUsage as u64)
            );
        }
        return "Unavailable".into();
    }
    #[cfg(not(windows))]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            let mut rss = None;
            let mut hwm = None;
            for line in status.lines() {
                if let Some(value) = line.strip_prefix("VmRSS:") {
                    rss = value
                        .split_whitespace()
                        .next()
                        .and_then(|kb| kb.parse::<u64>().ok());
                } else if let Some(value) = line.strip_prefix("VmHWM:") {
                    hwm = value
                        .split_whitespace()
                        .next()
                        .and_then(|kb| kb.parse::<u64>().ok());
                }
            }
            if let (Some(rss), Some(hwm)) = (rss, hwm) {
                return format!(
                    "{} resident, {} peak",
                    bytes(rss.saturating_mul(1024)),
                    bytes(hwm.saturating_mul(1024))
                );
            }
        }
        "Unavailable".into()
    }
}

/// What the computer's memory holds, used of total.
fn system_memory() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            return format!(
                "{} of {} used ({}%)",
                bytes(status.ullTotalPhys.saturating_sub(status.ullAvailPhys)),
                bytes(status.ullTotalPhys),
                status.dwMemoryLoad
            );
        }
        return "Unavailable".into();
    }
    #[cfg(not(windows))]
    {
        let Ok(info) = std::fs::read_to_string("/proc/meminfo") else {
            return "Unavailable".into();
        };
        let mut total = None;
        let mut available = None;
        for line in info.lines() {
            if let Some(value) = line.strip_prefix("MemTotal:") {
                total = value
                    .split_whitespace()
                    .next()
                    .and_then(|kb| kb.parse::<u64>().ok());
            } else if let Some(value) = line.strip_prefix("MemAvailable:") {
                available = value
                    .split_whitespace()
                    .next()
                    .and_then(|kb| kb.parse::<u64>().ok());
            }
        }
        if let (Some(total), Some(available)) = (total, available) {
            let total = total.saturating_mul(1024);
            let used = total.saturating_sub(available.saturating_mul(1024));
            return format!(
                "{} of {} used ({}%)",
                bytes(used),
                bytes(total),
                if total == 0 {
                    0
                } else {
                    used.saturating_mul(100) / total
                }
            );
        }
        "Unavailable".into()
    }
}

fn cache_size() -> String {
    #[cfg(feature = "discord")]
    {
        bytes(litecord_core::litecord_discord::cache::media_size())
    }
    #[cfg(not(feature = "discord"))]
    {
        "Nothing kept".into()
    }
}

/// Bytes as a person reads them: 16.4 MB, 1.2 GB.
fn bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * MB;
    const KB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn open_folder(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

/// A calculator over the four operations, parentheses and a power, with the
/// usual precedence. Percent is modulo, as a calculator's `%` key often is.
fn calculate(expression: &str) -> Result<f64, &'static str> {
    let mut parser = Parser {
        bytes: expression.as_bytes(),
        at: 0,
    };
    let value = parser.expression()?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err("That isn't an expression I can read.");
    }
    if !value.is_finite() {
        return Err("That comes out infinite.");
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.bytes.get(self.at).copied()
    }

    fn expression(&mut self) -> Result<f64, &'static str> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.at += 1;
                    value += self.term()?;
                }
                Some(b'-') => {
                    self.at += 1;
                    value -= self.term()?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn term(&mut self) -> Result<f64, &'static str> {
        let mut value = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.at += 1;
                    value *= self.power()?;
                }
                Some(b'/') => {
                    self.at += 1;
                    let divisor = self.power()?;
                    if divisor == 0.0 {
                        return Err("That divides by zero.");
                    }
                    value /= divisor;
                }
                Some(b'%') => {
                    self.at += 1;
                    let divisor = self.power()?;
                    if divisor == 0.0 {
                        return Err("That divides by zero.");
                    }
                    value %= divisor;
                }
                _ => return Ok(value),
            }
        }
    }

    fn power(&mut self) -> Result<f64, &'static str> {
        let value = self.unary()?;
        if self.peek() == Some(b'^') {
            self.at += 1;
            // Right-associative, as arithmetic's power is.
            return Ok(value.powf(self.power()?));
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<f64, &'static str> {
        match self.peek() {
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            Some(b'-') => {
                self.at += 1;
                Ok(-self.unary()?)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<f64, &'static str> {
        self.space();
        if self.peek() == Some(b'(') {
            self.at += 1;
            let value = self.expression()?;
            if self.peek() != Some(b')') {
                return Err("A parenthesis is left open.");
            }
            self.at += 1;
            return Ok(value);
        }
        let start = self.at;
        let mut dot = false;
        while let Some(byte) = self.bytes.get(self.at).copied() {
            if byte.is_ascii_digit() {
                self.at += 1;
            } else if byte == b'.' && !dot {
                dot = true;
                self.at += 1;
            } else {
                break;
            }
        }
        if self.at == start {
            return Err("That isn't an expression I can read.");
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| "That isn't an expression I can read.")?;
        text.parse::<f64>()
            .map_err(|_| "That isn't an expression I can read.")
    }
}

/// A result as the panel shows it: no trailing zeros, and whole numbers
/// without a decimal point.
fn format_number(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let rounded = (value * 1e10).round() / 1e10;
    if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
        return format!("{rounded:.0}");
    }
    let mut text = format!("{rounded:.10}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_with_precedence() {
        assert_eq!(calculate("2 * (3 + 4)").unwrap(), 14.0);
        assert_eq!(calculate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(calculate("2 ^ 3 ^ 2").unwrap(), 512.0);
        assert_eq!(calculate("-5 + 2").unwrap(), -3.0);
        assert_eq!(calculate("7 % 4").unwrap(), 3.0);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(calculate("2 +").is_err());
        assert!(calculate("(2 + 3").is_err());
        assert!(calculate("1 / 0").is_err());
        assert!(calculate("hello").is_err());
    }

    #[test]
    fn writes_whole_numbers_plainly() {
        assert_eq!(format_number(14.0), "14");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(1.0 / 3.0), "0.3333333333");
    }
}
