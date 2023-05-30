#[derive(Clone, Copy)]
pub enum Format {
    Bold,
    Italics,
    Underline,
    Strikethrough,
    Reverse,
    Color,
    Plain,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Bold => write!(f, "\x02"),
            Format::Italics => write!(f, "\x1d"),
            Format::Underline => write!(f, "\x1f"),
            Format::Strikethrough => write!(f, "\x1e"),
            Format::Reverse => write!(f, "\x12"),
            Format::Color => write!(f, "\x03"),
            Format::Plain => write!(f, "\x0f"),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Color {
    White,
    Black,
    Blue,
    Green,
    Red,
    Brown,
    Purple,
    Orange,
    Yellow,
    LightGreen,
    Teal,
    Cyan,
    LightBlue,
    Magenta,
    Gray,
    LightGray,
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::White => write!(f, "00"),
            Color::Black => write!(f, "01"),
            Color::Blue => write!(f, "02"),
            Color::Green => write!(f, "03"),
            Color::Red => write!(f, "04"),
            Color::Brown => write!(f, "05"),
            Color::Purple => write!(f, "06"),
            Color::Orange => write!(f, "07"),
            Color::Yellow => write!(f, "08"),
            Color::LightGreen => write!(f, "09"),
            Color::Teal => write!(f, "10"),
            Color::Cyan => write!(f, "11"),
            Color::LightBlue => write!(f, "12"),
            Color::Magenta => write!(f, "13"),
            Color::Gray => write!(f, "14"),
            Color::LightGray => write!(f, "15"),
        }
    }
}

pub struct Msg(String);

impl std::fmt::Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

impl Msg {
    pub fn new() -> Self {
        Self(String::new())
    }

    pub fn text(mut self, text: impl AsRef<str>) -> Self {
        self.0 += text.as_ref();
        self
    }
    pub fn color(mut self, color: Color) -> Self {
        self.0 += &Format::Color.to_string();
        self.0 += &color.to_string();
        self
    }
    pub fn format(mut self, format: Format) -> Self {
        self.0 += &format.to_string();
        self
    }
    pub fn reset(mut self) -> Self {
        self.0 += &Format::Plain.to_string();
        self
    }
}
