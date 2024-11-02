use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

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
