use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

pub fn encrypt(default_img: DynamicImage, img: DynamicImage) -> RgbaImage {
    let (width, height) = default_img.dimensions();
    let mut default_img = default_img.to_rgba8();

    let img = img.resize_exact(width, height, image::imageops::FilterType::Nearest).to_rgba8();

    for (x, y, px) in img.enumerate_pixels() {
        let default_px = default_img.get_pixel_mut(x, y);
        
        let r = (default_px[0] & 0xF0) | ((px[0] >> 4) & 0x0F);
        let g = (default_px[1] & 0xF0) | ((px[1] >> 4) & 0x0F);
        let b = (default_px[2] & 0xF0) | ((px[2] >> 4) & 0x0F);
        let a = default_px[3];

        let hidden_px = Rgba([r, g, b, a]);
        *default_px = hidden_px;
    }

    default_img
}


pub fn decrypt(base_img: DynamicImage, w: u32, h: u32) -> DynamicImage {
    let (width, height) = base_img.dimensions();
    let base_img =  base_img.to_rgba8();
    let mut extracted_img = RgbaImage::new(width, height);

    for (x, y, base_pixel) in base_img.enumerate_pixels() {
        let r = (base_pixel[0] & 0x0F) << 4;
        let g = (base_pixel[1] & 0x0F) << 4;
        let b = (base_pixel[2] & 0x0F) << 4;
        let a = 255;
        let hidden_pixel = Rgba([r, g, b, a]);
        extracted_img.put_pixel(x, y, hidden_pixel);
    }

    DynamicImage::ImageRgba8(extracted_img).resize_exact(w, h, image::imageops::FilterType::Nearest)
}
