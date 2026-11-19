//! Appearance: choosing a theme, and making and editing the person's own, and
//! the picture behind the app.

use super::*;

/// Litecord's own wallpaper, carried in the program so a fresh install has one.
const BUILT_IN_WALLPAPER: &[u8] = include_bytes!("../../ui/wallpaper.png");

/// The longest side a wallpaper is kept at, in pixels: a large photograph would
/// hold tens of megabytes for a window that shows far fewer.
const WALLPAPER_SIDE: u32 = 2560;

/// How dark the scrim over the wallpaper starts, as a percentage.
const DEFAULT_DIM: i32 = 35;

/// The darkest the scrim can make it, as a percentage: the picture stays
/// visible.
const MOST_DIM: i32 = 90;

impl Layout {
    /// Shows the wallpaper chosen in Settings > Appearance, or Litecord's own,
    /// as the person left it: shown or not, and how dark.
    pub(super) fn apply_wallpaper(&self, window: &LitecordWindow) {
        let (mut image, mut name, mut error) = (built_in_wallpaper(), String::new(), String::new());
        if let Some(path) = &self.wallpaper {
            match std::fs::read(path) {
                Ok(bytes) => match wallpaper_picture(&bytes) {
                    Some(chosen) => {
                        image = chosen;
                        name = file_name(path);
                    }
                    None => {
                        error = format!(
                            "Couldn't read {path} as a picture. Choose a PNG, JPEG, WebP, BMP or GIF image."
                        );
                    }
                },
                Err(_) => {
                    error = format!("Couldn't open {path}. Choose another image.");
                }
            }
        }
        window.set_wallpaper(image);
        window.set_wallpaper_name(name.into());
        window.set_wallpaper_error(error.into());
        window.set_wallpaper_on(self.wallpaper_on.unwrap_or(true));
        window.set_wallpaper_dim(self.dim());
    }

    /// The scrim over the wallpaper, as a percentage from 0 to [`MOST_DIM`].
    pub(super) fn dim(&self) -> i32 {
        self.wallpaper_dim.unwrap_or(DEFAULT_DIM).clamp(0, MOST_DIM)
    }
}

impl Adapter {
    /// Opens the system's file dialog on a picture and shows the one chosen at
    /// once. A file that isn't a picture says so, and leaves the wallpaper as
    /// it was.
    pub(super) fn choose_wallpaper(&mut self) {
        let weak = self.weak.clone();
        self.dialog(async move {
            let file = rfd::AsyncFileDialog::new()
                .set_title("Choose a wallpaper")
                .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                .pick_file()
                .await;
            let Some(file) = file else { return };
            let Some(adapter) = weak.upgrade() else {
                return;
            };
            let window = adapter.borrow().window.upgrade();
            let Some(window) = window else { return };
            adapter
                .borrow_mut()
                .set_wallpaper(&window, Some(file.path().to_owned()));
        });
    }

    /// Shows a picture as the wallpaper, from `path`, or Litecord's own when
    /// it's none, and keeps the choice.
    pub(super) fn set_wallpaper(&mut self, window: &LitecordWindow, path: Option<PathBuf>) {
        let Some(path) = path else {
            window.set_wallpaper(built_in_wallpaper());
            window.set_wallpaper_name(SharedString::default());
            window.set_wallpaper_error(SharedString::default());
            window.set_wallpaper_on(true);
            self.layout.wallpaper = None;
            self.layout.wallpaper_on = Some(true);
            self.layout.write(&self.root);
            return;
        };
        let picture = std::fs::read(&path)
            .ok()
            .and_then(|bytes| wallpaper_picture(&bytes));
        match picture {
            Some(image) => {
                window.set_wallpaper(image);
                window.set_wallpaper_name(file_name(&path.to_string_lossy()).into());
                window.set_wallpaper_error(SharedString::default());
                window.set_wallpaper_on(true);
                self.layout.wallpaper = Some(path.to_string_lossy().into_owned());
                self.layout.wallpaper_on = Some(true);
            }
            None => {
                window.set_wallpaper_error(
                    format!(
                        "Couldn't read {} as a picture. Choose a PNG, JPEG, WebP, BMP or GIF image.",
                        path.display()
                    )
                    .into(),
                );
            }
        }
        self.layout.write(&self.root);
    }

    /// Keeps the wallpaper shown or hidden, as the switch asks.
    pub(super) fn wallpaper_toggled(&mut self, window: &LitecordWindow, on: bool) {
        self.layout.wallpaper_on = Some(on);
        window.set_wallpaper_on(on);
        self.layout.write(&self.root);
    }

    /// Keeps how dark the scrim over the wallpaper is, once the slider's drag
    /// has ended.
    pub(super) fn wallpaper_dim_committed(&mut self, window: &LitecordWindow) {
        self.layout.wallpaper_dim = Some(window.get_wallpaper_dim().clamp(0, MOST_DIM));
        self.layout.write(&self.root);
    }
}

/// Decodes a picture as the window's wallpaper, shrinking one longer than
/// [`WALLPAPER_SIDE`] first. None when the bytes aren't a picture Litecord can
/// read.
pub(super) fn wallpaper_picture(bytes: &[u8]) -> Option<slint::Image> {
    let mut picture = image::load_from_memory(bytes).ok()?;
    if picture.width() > WALLPAPER_SIDE || picture.height() > WALLPAPER_SIDE {
        picture = picture.resize(
            WALLPAPER_SIDE,
            WALLPAPER_SIDE,
            image::imageops::FilterType::Triangle,
        );
    }
    let picture = picture.into_rgba8();
    let (width, height) = picture.dimensions();
    let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        picture.as_raw(),
        width,
        height,
    );
    Some(slint::Image::from_rgba8(pixels))
}

/// Litecord's own wallpaper. An empty image if the program carries no picture
/// it can read, which a build without one can.
fn built_in_wallpaper() -> slint::Image {
    wallpaper_picture(BUILT_IN_WALLPAPER).unwrap_or_default()
}

/// The last part of a path, as Settings names the chosen picture.
fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

impl Adapter {
    /// Shows the window in a theme, built in or the person's own, and keeps
    /// it chosen. False for an id Litecord doesn't know, which changes nothing.
    pub(super) fn choose_theme(&mut self, window: &LitecordWindow, id: &str) -> bool {
        let Some((id, chosen)) =
            theme::lookup(id, &self.layout.custom_themes).map(|(id, theme)| (id.to_owned(), theme))
        else {
            return false;
        };
        window.set_theme_id(id.as_str().into());
        self.set_theme(window, &chosen);
        self.layout.theme = Some(id);
        self.layout.write(&self.root);
        true
    }

    /// Shows the window in another theme's colours, building again what
    /// carries the old mention colour in its markup: message rows, the
    /// topic, pins and search results.
    pub(super) fn set_theme(&mut self, window: &LitecordWindow, theme: &theme::Theme) {
        if theme.apply(window) {
            self.restyle_mentions();
            self.refresh(window);
        }
    }

    /// Marks for building again, at the next refresh, what carries the
    /// mention colour in its markup.
    pub(super) fn restyle_mentions(&mut self) {
        self.restyle = true;
        self.topic_shown = None;
        self.models.pins.clear();
        self.models.search_results.clear();
    }

    /// Opens the editor on a copy of the theme shown, which the person can
    /// swap for any other.
    pub(super) fn new_theme(&mut self, window: &LitecordWindow) {
        let current = window.get_theme_id();
        let starts: Vec<_> = theme::starts(&self.layout.custom_themes).collect();
        let index = starts
            .iter()
            .position(|start| start.id == current.as_str())
            .unwrap_or(0);
        let names: Vec<SharedString> = starts.iter().map(|start| start.name.into()).collect();
        let (base, start) = (starts[index].base, starts[index].theme);
        window.set_theme_name(copy_name(starts[index].name).into());
        window.set_theme_starts(ModelRc::new(VecModel::from(names)));
        window.set_theme_start(index as i32);
        self.open_theme_editor(window, theme::Editor::new(None, base, start));
    }

    /// Opens the editor on one of the person's own themes.
    pub(super) fn edit_theme(&mut self, window: &LitecordWindow, id: &str) {
        let Some(custom) = self
            .layout
            .custom_themes
            .iter()
            .find(|custom| custom.id == id)
        else {
            return;
        };
        window.set_theme_name(custom.name.as_str().into());
        let editor = theme::Editor::new(Some(custom.id.clone()), custom.base, custom.theme);
        self.open_theme_editor(window, editor);
    }

    fn open_theme_editor(&mut self, window: &LitecordWindow, editor: theme::Editor) {
        window.set_theme_editing_saved(editor.editing.is_some());
        window.set_theme_dark(editor.theme.dark);
        window.set_theme_colours(editor.rows());
        window.set_theme_can_save(true);
        window.set_theme_editor(true);
        self.set_theme(window, &editor.theme);
        self.theme_editor = Some(editor);
    }

    /// The new theme starts again from another, and takes its name too
    /// unless the person named it.
    pub(super) fn theme_start_chosen(&mut self, window: &LitecordWindow, index: i32) {
        let custom = &self.layout.custom_themes;
        let start = |index: i32| {
            let index = usize::try_from(index).ok()?;
            theme::starts(custom)
                .nth(index)
                .map(|start| (start.base, start.theme, copy_name(start.name)))
        };
        let (Some(editor), Some((base, theme, name))) = (self.theme_editor.as_mut(), start(index))
        else {
            return;
        };
        let named = start(window.get_theme_start())
            .is_none_or(|(_, _, before)| before != window.get_theme_name().as_str());
        if !named {
            window.set_theme_name(name.into());
        }
        editor.restart(base, theme);
        window.set_theme_start(index);
        window.set_theme_dark(theme.dark);
        window.set_theme_can_save(true);
        self.set_theme(window, &theme);
    }

    /// A colour's field changed: the window shows it at once if it's a
    /// colour.
    pub(super) fn theme_colour_edited(&mut self, window: &LitecordWindow, index: i32, text: &str) {
        let Some(editor) = self.theme_editor.as_mut() else {
            return;
        };
        let changed = usize::try_from(index).is_ok_and(|index| editor.edit(index, text));
        window.set_theme_can_save(editor.valid());
        if changed {
            let theme = editor.theme;
            self.set_theme(window, &theme);
        }
    }

    pub(super) fn theme_dark_toggled(&mut self, window: &LitecordWindow, dark: bool) {
        let Some(editor) = self.theme_editor.as_mut() else {
            return;
        };
        editor.theme.dark = dark;
        let theme = editor.theme;
        self.set_theme(window, &theme);
    }

    /// Keeps the theme as edited, among the person's own, and chooses it.
    pub(super) fn save_theme(&mut self, window: &LitecordWindow) {
        let Some(editor) = self.theme_editor.take_if(|editor| editor.valid()) else {
            return;
        };
        let name = match window.get_theme_name().trim() {
            "" => "My theme".to_owned(),
            name => name.to_owned(),
        };
        let custom = &mut self.layout.custom_themes;
        let saved = editor
            .editing
            .as_ref()
            .and_then(|id| custom.iter_mut().find(|custom| custom.id == *id));
        let id = match saved {
            Some(saved) => {
                saved.name = name;
                saved.theme = editor.theme;
                saved.id.clone()
            }
            None => {
                let id = theme::new_id(custom);
                custom.push(theme::Custom {
                    id: id.clone(),
                    name,
                    base: editor.base,
                    theme: editor.theme,
                });
                id
            }
        };
        close_theme_editor(window);
        window.set_themes(theme::cards(&self.layout.custom_themes));
        self.choose_theme(window, &id);
    }

    /// Closes the editor and shows the theme from before it opened.
    pub(super) fn cancel_theme(&mut self, window: &LitecordWindow) {
        if self.theme_editor.take().is_some() {
            close_theme_editor(window);
            let (_, before) =
                theme::chosen(self.layout.theme.as_deref(), &self.layout.custom_themes);
            self.set_theme(window, &before);
        }
    }

    /// Deletes the theme being edited. If it was the chosen one, the Lite theme shows.
    pub(super) fn delete_theme(&mut self, window: &LitecordWindow) {
        let Some(editor) = self.theme_editor.take() else {
            return;
        };
        close_theme_editor(window);
        if let Some(id) = editor.editing {
            self.layout.custom_themes.retain(|custom| custom.id != id);
        }
        window.set_themes(theme::cards(&self.layout.custom_themes));
        let (id, _) = theme::chosen(self.layout.theme.as_deref(), &self.layout.custom_themes);
        let id = id.to_owned();
        self.choose_theme(window, &id);
    }
}

/// What a new theme copied from another is called until the person names it.
fn copy_name(name: &str) -> String {
    format!("{name} copy")
}

/// Drops the editor's models from the window, so they're held only while
/// it's open.
fn close_theme_editor(window: &LitecordWindow) {
    window.set_theme_editor(false);
    window.set_theme_colours(ModelRc::default());
    window.set_theme_starts(ModelRc::default());
}
