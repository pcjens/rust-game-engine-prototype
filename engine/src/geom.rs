/// A floating-point axis-aligned 2D rectangle.
pub struct Rect {
    /// The horizontal coordinate of the top-left corner of the rectangle.
    pub x: f32,
    /// The vertical coordinate of the top-left corner of the rectangle.
    pub y: f32,
    /// The width of the rectangle.
    pub w: f32,
    /// The height of the rectangle.
    pub h: f32,
}

impl Rect {
    /// Creates a new [`Rect`] from a given top-left corner and dimensions.
    pub const fn xywh(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
}
