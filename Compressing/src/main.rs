use image::{io::Reader as ImageReader, ImageOutputFormat};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

fn generate_low_quality_previews_for_directory(input_dir: &str,output_dir: &str, width: u32,height: u32,quality: u8,) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the output directory exists
    fs::create_dir_all(output_dir)?;

    // Iterate through all files in the input directory
    for entry in fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip non-files and non-image files
        if !path.is_file() {
            continue;
        }

        // Process each image
        if let Err(e) = process_image(&path, output_dir, width, height, quality) {
            eprintln!("Failed to process {}: {}", path.display(), e);
        }
    }

    println!("Previews generated in '{}'", output_dir);
    Ok(())
}

fn process_image(
    input_path: &Path,
    output_dir: &str,
    width: u32,
    height: u32,
    quality: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load the image
    let img = ImageReader::open(input_path)?.decode()?;

    // Resize the image
    let resized_img = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);

    // Construct the output file path
    let file_name = input_path
        .file_name()
        .ok_or("Failed to extract file name")?;
    let mut output_path = PathBuf::from(output_dir);
    // output_path.push(file_name);
    output_path.push(format!("{}_compressed.jpg", file_name.to_string_lossy()));
    // output_path.set_extension("jpg"); // Ensure the preview is saved as JPEG

    // Save the resized image as a JPEG with reduced quality
    let output_file = File::create(output_path)?;
    let mut writer = BufWriter::new(output_file);
    resized_img.write_to(&mut writer, ImageOutputFormat::Jpeg(quality))?;

    println!("Processed: {}", input_path.display());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Input directory containing images
    let input_dir = "my_images"; // Change this to your input directory
    // Output directory for previews
    let output_dir = "previews"; // Change this to your output directory
    // Desired preview dimensions and quality
    let width = 64;
    let height = 64;
    let quality = 25;

    generate_low_quality_previews_for_directory(input_dir, output_dir, width, height, quality)?;
    Ok(())
}
