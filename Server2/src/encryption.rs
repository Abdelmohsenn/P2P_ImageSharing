use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

pub fn encrypt(default_img: DynamicImage, img: DynamicImage) -> RgbaImage {
    let (width, height) = img.dimensions();
    let mut default_img = default_img.resize_exact(width, height, image::imageops::FilterType::Nearest).to_rgba8();
    let img = img.to_rgba8();
    let alpha = 0.1;
    let mask = 0x20;
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

pub fn decrypt(encrypted_img: DynamicImage) -> DynamicImage {
    let (width, height) = encrypted_img.dimensions();
    let encrypted_img =  encrypted_img.to_rgba8();
    let mut decrypted_img = RgbaImage::new(width, height);
    let alpha = 0.1;
    let mask = 0x20;
    for (x, y, base_px) in encrypted_img.enumerate_pixels() {
        let r = (base_px[0] & 0x0F) << 4;
        let g = (base_px[1] & 0x0F) << 4;
        let b = (base_px[2] & 0x0F) << 4;
        let a = (base_px[3] & 0x0F) << 4;
        let hidden_px = Rgba([r, g, b, a]);
        decrypted_img.put_pixel(x, y, hidden_px);
    }
    
    DynamicImage::ImageRgba8(decrypted_img)
}
