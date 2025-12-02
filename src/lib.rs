use image::{GenericImageView, GenericImage};
use ort::session::Session;

// Re-export internal structs if needed by main
pub use self::color_filter::{Color, ColorFilter, IntoHsvRange};
pub use self::charset::{Charset, CharsetRange};

mod color_filter {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum Color {
        Red, Blue, Green, Yellow, Orange, Purple, Cyan, Black, White, Gray,
    }

    impl<T: AsRef<str>> From<T> for Color {
        fn from(value: T) -> Self {
            match value.as_ref().to_ascii_lowercase().as_str() {
                "red" => Color::Red,
                "blue" => Color::Blue,
                "green" => Color::Green,
                "yellow" => Color::Yellow,
                "orange" => Color::Orange,
                "purple" => Color::Purple,
                "cyan" => Color::Cyan,
                "black" => Color::Black,
                "white" => Color::White,
                "gray" => Color::Gray,
                _ => panic!("unknown color: {}", value.as_ref()),
            }
        }
    }

    pub trait IntoHsvRange {
        fn into_hsv_ranges(self) -> Vec<((u8, u8, u8), (u8, u8, u8))>;
    }

    impl IntoHsvRange for Color {
        fn into_hsv_ranges(self) -> Vec<((u8, u8, u8), (u8, u8, u8))> {
            match self {
                Color::Red => vec![((0, 50, 50), (10, 255, 255)), ((170, 50, 50), (180, 255, 255))],
                Color::Blue => vec![((100, 50, 50), (140, 255, 255))],
                Color::Green => vec![((40, 50, 50), (80, 255, 255))],
                Color::Yellow => vec![((20, 50, 50), (40, 255, 255))],
                Color::Orange => vec![((10, 50, 50), (20, 255, 255))],
                Color::Purple => vec![((140, 50, 50), (170, 255, 255))],
                Color::Cyan => vec![((80, 50, 50), (100, 255, 255))],
                Color::Black => vec![((0, 0, 0), (180, 255, 30))],
                Color::White => vec![((0, 0, 200), (180, 30, 255))],
                Color::Gray => vec![((0, 0, 30), (180, 30, 200))],
            }
        }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(untagged)]
    pub enum ColorFilter {
        HSVRanges(Vec<((u8, u8, u8), (u8, u8, u8))>),
        ColorRanges(Vec<Color>),
        Color(Color),
    }

    impl ColorFilter {
        pub fn filter<I>(&self, image: I) -> anyhow::Result<image::DynamicImage>
        where
            I: AsRef<[u8]>,
        {
            let image = image::load_from_memory(image.as_ref())?.to_rgb8();
            let (width, height) = image.dimensions();
            let mut array = ndarray::Array3::<u8>::zeros((height as usize, width as usize, 3));

            for (x, y, pixel) in image.enumerate_pixels() {
                array[[y as usize, x as usize, 0]] = pixel[0];
                array[[y as usize, x as usize, 1]] = pixel[1];
                array[[y as usize, x as usize, 2]] = pixel[2];
            }

            let mut hsv = ndarray::Array3::<u8>::zeros((height as usize, width as usize, 3));

            for y in 0..height as usize {
                for x in 0..width as usize {
                    let r = array[[y, x, 0]] as f32 / 255.0;
                    let g = array[[y, x, 1]] as f32 / 255.0;
                    let b = array[[y, x, 2]] as f32 / 255.0;
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let delta = max - min;
                    let s = if max == 0.0 { 0.0 } else { delta / max };
                    let mut h_deg = if delta == 0.0 {
                        0.0
                    } else if max == r {
                        60.0 * (((g - b) / delta) % 6.0)
                    } else if max == g {
                        60.0 * (((b - r) / delta) + 2.0)
                    } else {
                        60.0 * (((r - g) / delta) + 4.0)
                    };

                    if h_deg < 0.0 { h_deg += 360.0; }

                    hsv[[y, x, 0]] = (h_deg / 2.0).round().min(180.0) as u8;
                    hsv[[y, x, 1]] = (s * 255.0).round().min(255.0) as u8;
                    hsv[[y, x, 2]] = (max * 255.0).round().min(255.0) as u8;
                }
            }

            let mut mask = ndarray::Array2::<bool>::from_elem((height as usize, width as usize), false);

            let ranges = match self {
                ColorFilter::HSVRanges(v) => v.clone(),
                ColorFilter::ColorRanges(v) => v.iter().flat_map(|v| v.clone().into_hsv_ranges()).collect(),
                ColorFilter::Color(v) => v.clone().into_hsv_ranges(),
            };

            for (lower, upper) in ranges {
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let h = hsv[[y, x, 0]];
                        let s = hsv[[y, x, 1]];
                        let v = hsv[[y, x, 2]];

                        if h >= lower.0 && h <= upper.0 && s >= lower.1 && s <= upper.1 && v >= lower.2 && v <= upper.2 {
                            mask[[y, x]] = true;
                        }
                    }
                }
            }

            let mut result: image::RgbImage = image::ImageBuffer::new(width, height);
            for y in 0..height {
                for x in 0..width {
                    if mask[[y as usize, x as usize]] {
                        result.put_pixel(x, y, image::Rgb([array[[y as usize, x as usize, 0]], array[[y as usize, x as usize, 1]], array[[y as usize, x as usize, 2]]]));
                    } else {
                        result.put_pixel(x, y, image::Rgb([255, 255, 255]));
                    }
                }
            }
            Ok(image::DynamicImage::ImageRgb8(result))
        }
    }
    
    impl From<&str> for ColorFilter { fn from(v: &str) -> Self { Color::from(v).into_hsv_ranges().into() } }
    impl From<Vec<((u8, u8, u8), (u8, u8, u8))>> for ColorFilter { fn from(v: Vec<((u8, u8, u8), (u8, u8, u8))>) -> Self { ColorFilter::HSVRanges(v) } }
    // ... omitting excessive implementation boilerplate for brevity where feasible
}

mod charset {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Charset {
        pub word: bool,
        pub image: [i64; 2],
        pub channel: i64,
        pub charset: Vec<String>,
    }
    
    impl std::str::FromStr for Charset {
        type Err = serde_json::Error;
        fn from_str(s: &str) -> Result<Self, Self::Err> { serde_json::from_str(s) }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub enum CharsetRange {
        Digit, Lowercase, Uppercase, LowercaseUppercase, LowercaseDigit, UppercaseDigit, LowercaseUppercaseDigit,
        DefaultCharsetLowercaseUppercaseDigit, Other(String), Charset(Vec<String>),
    }

    impl From<i32> for CharsetRange {
        fn from(value: i32) -> Self {
             match value {
                0 => Self::Digit, 1 => Self::Lowercase, 2 => Self::Uppercase, 3 => Self::LowercaseUppercase,
                4 => Self::LowercaseDigit, 5 => Self::UppercaseDigit, 6 => Self::LowercaseUppercaseDigit,
                7 => Self::DefaultCharsetLowercaseUppercaseDigit, _ => panic!("invalid range"),
            }
        }
    }
    
    impl From<&str> for CharsetRange { fn from(v: &str) -> Self { Self::Other(v.to_string()) } }
    impl From<String> for CharsetRange { fn from(v: String) -> Self { Self::Other(v) } }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BBox {
    pub x1: u32, pub y1: u32, pub x2: u32, pub y2: u32,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SlideBBox {
    pub target_x: u32, pub target_y: u32,
    pub x1: u32, pub y1: u32, pub x2: u32, pub y2: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CharacterProbability {
    pub text: Option<String>,
    pub charset: Vec<String>,
    pub probability: Vec<Vec<f32>>,
    pub confidence: Option<f64>,
}

impl CharacterProbability {
    pub fn get_text(&mut self) -> &str {
        self.text.get_or_insert_with(|| {
            let mut s = String::new();
            for i in &self.probability {
                let (n, _) = i.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap();
                s += &self.charset[n];
            }
            s
        })
    }
}

lazy_static::lazy_static! {
    static ref _STATIC: (Vec<u32>, Vec<u32>) = {
        let mut grids = Vec::new();
        let mut expanded_strides = Vec::new();
        let hsizes = STRIDES.iter().map(|v| MODEL_HEIGHT / v).collect::<Vec<_>>();
        let wsizes = STRIDES.iter().map(|v| MODEL_WIDTH / v).collect::<Vec<_>>();
        for (i, v) in STRIDES.iter().enumerate() {
            let hsize = hsizes[i];
            let wsize = wsizes[i];
            let mut grid = vec![0; (hsize * wsize * 2) as usize];
            for i in 0..hsize {
                for j in 0..wsize {
                    let index = ((i * wsize + j) * 2) as usize;
                    grid[index] = j;
                    grid[index + 1] = i;
                }
            }
            grids.extend(grid);
            expanded_strides.extend(vec![*v; (hsize * wsize) as usize]);
        }
        (grids, expanded_strides)
    };
    static ref GRIDS: Vec<u32> = unsafe { std::mem::transmute_copy(&_STATIC.0) };
    static ref EXPANDED_STRIDES: Vec<u32> =  unsafe { std::mem::transmute_copy(&_STATIC.1) };
}

const NMS_THR: f32 = 0.45;
const SCORE_THR: f32 = 0.1;
const MODEL_WIDTH: u32 = 416;
const MODEL_HEIGHT: u32 = 416;
const STRIDES: [u32; 3] = [8, 16, 32];

pub struct Ddddocr<'a> {
    diy: bool,
    session: std::sync::Mutex<Session>,
    charset: Option<std::borrow::Cow<'a, Charset>>,
    charset_range: Vec<String>,
}

unsafe impl<'a> Send for Ddddocr<'a> {}
unsafe impl<'a> Sync for Ddddocr<'a> {}

pub fn is_diy(model: &[u8]) -> bool {
    let sha = sha256::digest(model);
    sha != "33b5cd351ee94e73a6bf8fa18c415ed8b819b3ffd342e267c30d8ad8334e34e8"
        && sha != "b8f2ad9cbc1f2e3922a6cb9459e30824e7e2467f3fb4fd61420640e34ea0bf68"
}

fn png_rgba_black_preprocess(image: &image::DynamicImage) -> image::DynamicImage {
    let (width, height) = image.dimensions();
    let mut new_image = image::DynamicImage::new_rgb8(width, height);
    for y in 0..height {
        for x in 0..width {
            let p = image.get_pixel(x, y);
            let p = if p[3] == 0 { image::Rgba([255, 255, 255, 255]) } else { p };
            new_image.put_pixel(x, y, image::Rgba([p[0], p[1], p[2], 255]));
        }
    }
    new_image
}

impl<'a> Ddddocr<'a> {
    pub fn new<MODEL>(model: MODEL, charset: Charset) -> anyhow::Result<Self>
    where MODEL: AsRef<[u8]> {
        Ok(Self {
            diy: is_diy(model.as_ref()),
            session: std::sync::Mutex::new(Session::builder()?.commit_from_memory(model.as_ref())?),
            charset: Some(std::borrow::Cow::Owned(charset)),
            charset_range: Vec::new(),
        })
    }

    pub fn new_det<MODEL>(model: MODEL) -> anyhow::Result<Self> 
    where MODEL: AsRef<[u8]> {
        Ok(Self {
            diy: is_diy(model.as_ref()),
            session: std::sync::Mutex::new(Session::builder()?.commit_from_memory(model.as_ref())?),
            charset: None,
            charset_range: Vec::new(),
        })
    }

    pub fn calc_ranges<R>(&self, ranges: R) -> Vec<String> 
    where R: Into<CharsetRange> {
        let charset = match ranges.into() {
            CharsetRange::Digit => "0123456789".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::Lowercase => "abcdefghijklmnopqrstuvwxyz".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::Uppercase => "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::LowercaseUppercase => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::LowercaseDigit => "abcdefghijklmnopqrstuvwxyz0123456789".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::UppercaseDigit => "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::LowercaseUppercaseDigit => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::Other(v) => v.chars().map(|c| c.to_string()).collect::<Vec<_>>(),
            CharsetRange::Charset(v) => return v,
            CharsetRange::DefaultCharsetLowercaseUppercaseDigit => {
                 let delete_range: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().collect();
                 self.charset.as_ref().expect("OCR model required").charset.iter()
                     .filter(|v| v.chars().all(|c| !delete_range.contains(&c)))
                     .cloned().collect()
            }
        };
        let mut unique: Vec<String> = charset.into_iter().collect::<std::collections::HashSet<_>>().into_iter().collect();
        unique.push("".to_string());
        unique
    }

    pub fn set_ranges<R>(&mut self, ranges: R) where R: Into<CharsetRange> {
        self.charset_range = self.calc_ranges(ranges)
    }

    pub fn classification_probability_with_options<I>(&self, image: I, png_fix: bool, filter: Option<ColorFilter>, ranges: Option<CharsetRange>) -> anyhow::Result<CharacterProbability>
    where I: AsRef<[u8]> {
        let charset_ranges = match ranges {
            Some(v) => self.calc_ranges(v),
            None => self.charset_range.clone(),
        };
        
        let image = match filter {
            Some(v) => v.filter(image.as_ref())?,
            None => image::load_from_memory(image.as_ref())?,
        };

        let charset_conf = self.charset.as_ref().unwrap();
        let resize = charset_conf.image;
        let channel = charset_conf.channel as usize;

        let image = if resize[0] == -1 {
             let w = if charset_conf.word { resize[1] as u32 } else { image.width() * resize[1] as u32 / image.height() };
             image.resize_exact(w, resize[1] as u32, image::imageops::FilterType::Lanczos3)
        } else {
            image.resize_exact(resize[0] as u32, resize[1] as u32, image::imageops::FilterType::Lanczos3)
        };

        let image_bytes = if channel == 1 {
            image.to_luma8().as_raw().clone()
        } else if png_fix {
            png_rgba_black_preprocess(&image).to_rgb8().as_raw().clone()
        } else {
            image.to_rgb8().as_raw().clone()
        };

        let width = image.width() as usize;
        let height = image.height() as usize;
        let image_arr = ndarray::Array::from_shape_vec((height, width, channel), image_bytes)?;
        // Transpose to (channel, height, width)
        let image_arr = image_arr.permuted_axes([2, 0, 1]);
        
        let mut tensor = ndarray::Array::from_shape_vec((1, channel, height, width), vec![0f32; channel*height*width])?;

        for i in 0..height {
            for j in 0..width {
                 let now = image_arr[[0, i, j]] as f32;
                 if self.diy {
                      if channel == 1 {
                          tensor[[0, 0, i, j]] = ((now / 255.0) - 0.456) / 0.224;
                      } else {
                          tensor[[0, 0, i, j]] = ((image_arr[[0, i, j]] as f32 / 255.0) - 0.485) / 0.229;
                          tensor[[0, 1, i, j]] = ((image_arr[[1, i, j]] as f32 / 255.0) - 0.456) / 0.224;
                          tensor[[0, 2, i, j]] = ((image_arr[[2, i, j]] as f32 / 255.0) - 0.406) / 0.225;
                      }
                 } else {
                     tensor[[0, 0, i, j]] = ((now / 255.0) - 0.5) / 0.5;
                 }
            }
        }

        let shape = tensor.shape().to_vec();
        let data = tensor.into_raw_vec_and_offset().0;
        let input_value = ort::value::Value::from_array((shape, data))?;
        let mut session = self.session.lock().unwrap();
        let outputs = session.run(ort::inputs![input_value])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let shape_usize: Vec<usize> = shape.iter().map(|&v| v as usize).collect();
        let output = ndarray::ArrayView::from_shape(shape_usize, data)?;
        
        // Softmax? Reference code implementation:
        let output = output.mapv(f32::exp) / output.mapv(f32::exp).sum();
        let output_sum = output.sum_axis(ndarray::Axis(2));
        
        // Normalize
        let mut probability = ndarray::Array::<f32, _>::zeros(output.raw_dim());
        for i in 0..output.shape()[0] {
            let mut a = probability.slice_mut(ndarray::s![i, .., ..]);
            let b = output.slice(ndarray::s![i, .., ..]);
            let c = output_sum.slice(ndarray::s![i, ..]);
            a.assign(&(&b / &c));
        }
        
        let probability = probability.index_axis(ndarray::Axis(1), 0);
        let mut result = Vec::new();
        for row in probability.axis_iter(ndarray::Axis(0)) {
             result.push(row.into_diag().to_vec());
        }
        
        if charset_ranges.is_empty() {
            Ok(CharacterProbability { text: None, charset: charset_conf.charset.clone(), probability: result, confidence: None })
        } else {
             let mut indices = Vec::new();
             for r in &charset_ranges {
                 indices.push(charset_conf.charset.iter().position(|c| c == r).unwrap_or(usize::MAX));
             }
             let mut filtered = Vec::new();
             for item in &result {
                 let mut inner = Vec::new();
                 for &idx in &indices {
                     if idx != usize::MAX { inner.push(item[idx]); } else { inner.push(-1.0); }
                 }
                 filtered.push(inner);
             }
             Ok(CharacterProbability { text: None, charset: charset_ranges, probability: filtered, confidence: None })
        }
    }

    pub fn classification_with_options<I>(&self, image: I, png_fix: bool, filter: Option<ColorFilter>) -> anyhow::Result<String>
    where I: AsRef<[u8]> {
        let prob = self.classification_probability_with_options(image, png_fix, filter, None)?;
        let mut p = prob.clone();
        Ok(p.get_text().to_string())
    }

    pub fn detection<I>(&self, image: I) -> anyhow::Result<Vec<BBox>> where I: AsRef<[u8]> {
         #[derive(Debug, Clone, Copy)] struct ScoresBBox { scores: f32, x1: f32, y1: f32, x2: f32, y2: f32 }
         let original = image::load_from_memory(image.as_ref())?;
         let (orig_w, orig_h) = original.dimensions();
         let x_scale = MODEL_WIDTH as f32 / orig_w as f32;
         let y_scale = MODEL_HEIGHT as f32 / orig_h as f32;
         let gain = x_scale.min(y_scale);
         let resize_w = (orig_w as f32 * gain) as u32;
         let resize_h = (orig_h as f32 * gain) as u32;
         
         let image = original.resize_exact(resize_w, resize_h, image::imageops::FilterType::Triangle).to_rgb8();
         let mut canvas = image::ImageBuffer::from_pixel(MODEL_WIDTH, MODEL_HEIGHT, image::Rgb([114, 114, 114]));
         image::imageops::overlay(&mut canvas, &image, 0, 0);
         
         let mut input_tensor = ndarray::Array::from_shape_vec((1, 3, MODEL_HEIGHT as usize, MODEL_WIDTH as usize), vec![0f32; 3 * MODEL_HEIGHT as usize * MODEL_WIDTH as usize])?;
         
         for (x, y, p) in canvas.enumerate_pixels() {
             // Reference: x and y might be swapped in tensor assignment in original code?
             // Original: input_tensor[[0, 0, i as usize, j as usize]] = now[0] as f32; where i is x loop, j is y loop.
             // So tensor is (y, x)? No, usually (channel, height, width). 
             // Original code: for i in 0..image.width() { for j in 0..image.height() { ... [j, i] ... } }
             // So it assigns [y, x]. Correct.
             input_tensor[[0, 0, y as usize, x as usize]] = p[0] as f32;
             input_tensor[[0, 1, y as usize, x as usize]] = p[1] as f32;
             input_tensor[[0, 2, y as usize, x as usize]] = p[2] as f32;
         }

         let shape = input_tensor.shape().to_vec();
         let data = input_tensor.into_raw_vec_and_offset().0;
         let input_value = ort::value::Value::from_array((shape, data))?;
         let mut session = self.session.lock().unwrap();
         let outputs = session.run(ort::inputs![input_value])?;
         let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
         let shape_usize: Vec<usize> = shape.iter().map(|&v| v as usize).collect();
         let output = ndarray::ArrayView::from_shape(shape_usize, data)?;
         
         let mut boxes = Vec::new();
         let num_boxes = output.len() / 6;
         for i in 0..num_boxes {
             let score = output[[0, i, 4]] * output[[0, i, 5]];
             if score < SCORE_THR { continue; }
             let x1 = (output[[0, i, 0]] + GRIDS[i * 2] as f32) * EXPANDED_STRIDES[i] as f32;
             let y1 = (output[[0, i, 1]] + GRIDS[i * 2 + 1] as f32) * EXPANDED_STRIDES[i] as f32;
             let x2 = output[[0, i, 2]].exp() * EXPANDED_STRIDES[i] as f32;
             let y2 = output[[0, i, 3]].exp() * EXPANDED_STRIDES[i] as f32;
             
             boxes.push(ScoresBBox {
                 scores: score,
                 x1: (x1 - x2 / 2.0) / gain,
                 y1: (y1 - y2 / 2.0) / gain,
                 x2: (x1 + x2 / 2.0) / gain,
                 y2: (y1 + y2 / 2.0) / gain,
             });
         }
         
         // NMS
         boxes.sort_by(|a, b| b.scores.partial_cmp(&a.scores).unwrap());
         let mut result = Vec::new();
         while !boxes.is_empty() {
             let current = boxes.remove(0);
             result.push(current);
             boxes.retain(|b| {
                 let xx1 = current.x1.max(b.x1);
                 let yy1 = current.y1.max(b.y1);
                 let xx2 = current.x2.min(b.x2);
                 let yy2 = current.y2.min(b.y2);
                 let w = (xx2 - xx1 + 1.0).max(0.0);
                 let h = (yy2 - yy1 + 1.0).max(0.0);
                 let inter = w * h;
                 let area1 = (current.x2 - current.x1 + 1.0) * (current.y2 - current.y1 + 1.0);
                 let area2 = (b.x2 - b.x1 + 1.0) * (b.y2 - b.y1 + 1.0);
                 let iou = inter / (area1 + area2 - inter);
                 iou <= NMS_THR
             });
         }

         Ok(result.into_iter().map(|b| BBox {
             x1: b.x1.max(0.0).min(orig_w as f32 - 1.0) as u32,
             y1: b.y1.max(0.0).min(orig_h as f32 - 1.0) as u32,
             x2: b.x2.max(0.0).min(orig_w as f32 - 1.0) as u32,
             y2: b.y2.max(0.0).min(orig_h as f32 - 1.0) as u32,
         }).collect())
    }
}

pub fn slide_match<I1, I2>(target: I1, bg: I2) -> anyhow::Result<SlideBBox> 
where I1: AsRef<[u8]>, I2: AsRef<[u8]> {
    let target = image::load_from_memory(target.as_ref())?;
    let bg = image::load_from_memory(bg.as_ref())?;
    anyhow::ensure!(bg.width() >= target.width() && bg.height() >= target.height(), "bg too small");
    
    let target = target.to_rgba8();
    let (w, h) = target.dimensions();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0, 0);
    
    for x in 0..w {
        for y in 0..h {
             if target[(x, y)][3] != 0 {
                 if x < min_x { min_x = x; }
                 if y < min_y { min_y = y; }
                 if x > max_x { max_x = x; }
                 if y > max_y { max_y = y; }
             }
        }
    }
    
    let crop = if min_x > max_x { target.clone() } else {
         image::imageops::crop_imm(&target, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1).to_image()
    };
    
    let t_edge = imageproc::edges::canny(&image::imageops::grayscale(&crop), 100.0, 200.0);
    let b_edge = imageproc::edges::canny(&bg.to_luma8(), 100.0, 200.0);
    let res = imageproc::template_matching::match_template(&b_edge, &t_edge, imageproc::template_matching::MatchTemplateMethod::CrossCorrelationNormalized);
    let extremes = imageproc::template_matching::find_extremes(&res);
    
    Ok(SlideBBox {
        target_x: min_x, target_y: min_y,
        x1: extremes.max_value_location.0, y1: extremes.max_value_location.1,
        x2: extremes.max_value_location.0 + t_edge.width(), y2: extremes.max_value_location.1 + t_edge.height()
    })
}

pub fn simple_slide_match<I1, I2>(target: I1, bg: I2) -> anyhow::Result<SlideBBox> 
where I1: AsRef<[u8]>, I2: AsRef<[u8]> {
    let target = image::load_from_memory(target.as_ref())?;
    let bg = image::load_from_memory(bg.as_ref())?;
    
    let t_edge = imageproc::edges::canny(&target.to_luma8(), 100.0, 200.0);
    let b_edge = imageproc::edges::canny(&bg.to_luma8(), 100.0, 200.0);
    let res = imageproc::template_matching::match_template(&b_edge, &t_edge, imageproc::template_matching::MatchTemplateMethod::CrossCorrelationNormalized);
    let extremes = imageproc::template_matching::find_extremes(&res);
    
    Ok(SlideBBox {
        target_x: 0, target_y: 0,
        x1: extremes.max_value_location.0, y1: extremes.max_value_location.1,
        x2: extremes.max_value_location.0 + t_edge.width(), y2: extremes.max_value_location.1 + t_edge.height()
    })
}

pub fn slide_comparison<I1, I2>(target: I1, bg: I2) -> anyhow::Result<(u32, u32)> 
where I1: AsRef<[u8]>, I2: AsRef<[u8]> {
    let t = image::load_from_memory(target.as_ref())?.to_rgb8();
    let b = image::load_from_memory(bg.as_ref())?.to_rgb8();
    anyhow::ensure!(t.dimensions() == b.dimensions(), "dimensions mismatch");
    
    let diff_img = image::RgbImage::from_fn(t.width(), t.height(), |x, y| {
        let p1 = t.get_pixel(x, y);
        let p2 = b.get_pixel(x, y);
        // Simple diff
        if (p1[0] as i16 - p2[0] as i16).abs() > 80 || 
           (p1[1] as i16 - p2[1] as i16).abs() > 80 || 
           (p1[2] as i16 - p2[2] as i16).abs() > 80 {
            image::Rgb([255, 255, 255])
        } else {
            image::Rgb([0, 0, 0])
        }
    });
    
    for x in 0..diff_img.width() {
        let mut count = 0;
        for y in 0..diff_img.height() {
            if diff_img[(x, y)] == image::Rgb([255, 255, 255]) { count += 1; }
            if count >= 5 { return Ok((x, y.saturating_sub(5))); }
        }
    }
    
    Ok((0, 0))
}
