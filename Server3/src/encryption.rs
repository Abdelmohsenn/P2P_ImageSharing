use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

pub fn encrypt(default_img: DynamicImage, img: DynamicImage) -> RgbaImage {
    let (width, height) = img.dimensions();
    let mut default_img = default_img
        .resize_exact(width, height, image::imageops::FilterType::Nearest)
        .to_rgba8();
    let img = img.to_rgba8();

    for (x, y, px) in img.enumerate_pixels() {
        let default_px = default_img.get_pixel_mut(x, y);

        let r = (default_px[0] & 0xF0) | ((px[0] >> 4) & 0x0F);
        let g = (default_px[1] & 0xF0) | ((px[1] >> 4) & 0x0F);
        let b = (default_px[2] & 0xF0) | ((px[2] >> 4) & 0x0F);
        let a = (default_px[3] & 0xF0) | ((px[3] >> 4) & 0x0F);

        let hidden_px = Rgba([r, g, b, a]);
        *default_px = hidden_px;
    }

    default_img
}
