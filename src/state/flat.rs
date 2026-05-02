use rkyv::Archive;

use super::{
    CanvasObject, CanvasShape, CanvasShapeType, CanvasState, CanvasStroke, CanvasText, Color32,
    Pos2, StrokeWidth,
};

// ===== Flat data types for rkyv canvas serialization =====

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct CanvasStateFlat {
    pub objects: Vec<CanvasObjectFlat>,
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum CanvasObjectFlat {
    Stroke(StrokeFlat),
    Text(TextFlat),
    Shape(ShapeFlat),
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct StrokeFlat {
    pub points: Vec<[f32; 2]>,
    pub width: StrokeWidthFlat,
    pub color: [u8; 4],
    pub base_width: f32,
    pub rot: f32,
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum StrokeWidthFlat {
    Fixed(f32),
    Dynamic(Vec<f32>),
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct TextFlat {
    pub text: String,
    pub pos: [f32; 2],
    pub color: [u8; 4],
    pub font_size: f32,
    pub rot: f32,
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct ShapeFlat {
    pub shape_type: ShapeTypeFlat,
    pub pos: [f32; 2],
    pub size: f32,
    pub color: [u8; 4],
    pub rotation: f32,
}

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum ShapeTypeFlat {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

// Conversions between CanvasState and flat types

impl From<&CanvasState> for CanvasStateFlat {
    fn from(state: &CanvasState) -> Self {
        CanvasStateFlat {
            objects: state
                .objects
                .iter()
                .filter_map(|obj| match obj {
                    CanvasObject::Stroke(s) => Some(CanvasObjectFlat::Stroke(StrokeFlat {
                        points: s.points.iter().map(|p| [p.x, p.y]).collect(),
                        width: match &s.width {
                            StrokeWidth::Fixed(w) => StrokeWidthFlat::Fixed(*w),
                            StrokeWidth::Dynamic(v) => StrokeWidthFlat::Dynamic(v.clone()),
                        },
                        color: [s.color.r(), s.color.g(), s.color.b(), s.color.a()],
                        base_width: s.base_width,
                        rot: s.rot,
                    })),
                    CanvasObject::Text(t) => Some(CanvasObjectFlat::Text(TextFlat {
                        text: t.text.clone(),
                        pos: [t.pos.x, t.pos.y],
                        color: [t.color.r(), t.color.g(), t.color.b(), t.color.a()],
                        font_size: t.font_size,
                        rot: t.rot,
                    })),
                    CanvasObject::Shape(s) => Some(CanvasObjectFlat::Shape(ShapeFlat {
                        shape_type: match s.shape_type {
                            CanvasShapeType::Line => ShapeTypeFlat::Line,
                            CanvasShapeType::Arrow => ShapeTypeFlat::Arrow,
                            CanvasShapeType::Rectangle => ShapeTypeFlat::Rectangle,
                            CanvasShapeType::Triangle => ShapeTypeFlat::Triangle,
                            CanvasShapeType::Circle => ShapeTypeFlat::Circle,
                        },
                        pos: [s.pos.x, s.pos.y],
                        size: s.size,
                        color: [s.color.r(), s.color.g(), s.color.b(), s.color.a()],
                        rotation: s.rotation,
                    })),
                    CanvasObject::Image(_) => None,
                })
                .collect(),
        }
    }
}

impl<'a> From<&'a ArchivedCanvasStateFlat> for CanvasState {
    fn from(archived: &'a ArchivedCanvasStateFlat) -> Self {
        CanvasState {
            objects: archived
                .objects
                .iter()
                .map(|obj| match obj {
                    ArchivedCanvasObjectFlat::Stroke(s) => CanvasObject::Stroke(CanvasStroke {
                        points: s
                            .points
                            .iter()
                            .map(|p| Pos2::new(p[0].into(), p[1].into()))
                            .collect(),
                        width: match &s.width {
                            ArchivedStrokeWidthFlat::Fixed(w) => StrokeWidth::Fixed((*w).into()),
                            ArchivedStrokeWidthFlat::Dynamic(v) => {
                                StrokeWidth::Dynamic(v.iter().map(|&x| x.into()).collect())
                            }
                        },
                        color: Color32::from_rgba_unmultiplied(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        ),
                        base_width: s.base_width.into(),
                        rot: s.rot.into(),
                    }),
                    ArchivedCanvasObjectFlat::Text(t) => CanvasObject::Text(CanvasText {
                        text: t.text.as_str().to_string(),
                        pos: Pos2::new(t.pos[0].into(), t.pos[1].into()),
                        color: Color32::from_rgba_unmultiplied(
                            t.color[0], t.color[1], t.color[2], t.color[3],
                        ),
                        font_size: t.font_size.into(),
                        rot: t.rot.into(),
                    }),
                    ArchivedCanvasObjectFlat::Shape(s) => CanvasObject::Shape(CanvasShape {
                        shape_type: match s.shape_type {
                            ArchivedShapeTypeFlat::Line => CanvasShapeType::Line,
                            ArchivedShapeTypeFlat::Arrow => CanvasShapeType::Arrow,
                            ArchivedShapeTypeFlat::Rectangle => CanvasShapeType::Rectangle,
                            ArchivedShapeTypeFlat::Triangle => CanvasShapeType::Triangle,
                            ArchivedShapeTypeFlat::Circle => CanvasShapeType::Circle,
                        },
                        pos: Pos2::new(s.pos[0].into(), s.pos[1].into()),
                        size: s.size.into(),
                        color: Color32::from_rgba_unmultiplied(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        ),
                        rotation: s.rotation.into(),
                    }),
                })
                .collect(),
        }
    }
}
