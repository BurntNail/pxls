use anyhow::bail;
use image::{ColorType, DynamicImage, GenericImage, GenericImageView, ImageReader, Pixel, Rgb};

fn main() -> anyhow::Result<()> {
    println!("reading image");
    let image = ImageReader::open({
        let Some(image) = std::env::args().nth(1) else {
            bail!("unable to find image argument");
        };
        image
    })?.decode()?;
    println!("decoded image");

    println!("getting palette");
    let av_px_colours = get_av_px_colours(&image);
    println!("found palette");
    println!("getting output");
    let output = convert_to_palette(&image, av_px_colours);
    println!("saved output");

    output.save("output.jpg")?;

    Ok(())
}


fn get_av_px_colours (image: &DynamicImage) -> Vec<Rgb<u8>> {
    const LENGTH_SECTION_SIZE: u32 = 3;
    let (width, height) = image.dimensions();
    let (width_chunk_size, height_chunk_size) = (width / LENGTH_SECTION_SIZE, height / LENGTH_SECTION_SIZE);

    let mut av_px_colours = Vec::with_capacity((LENGTH_SECTION_SIZE * LENGTH_SECTION_SIZE) as usize);
    for chunk_x in 0..LENGTH_SECTION_SIZE {
        println!("\tprocessing x chunk {}", chunk_x + 1);
        for chunk_y in 0..LENGTH_SECTION_SIZE {
            let (mut accum_r, mut accum_g, mut accum_b) = (0, 0, 0);

            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let [r, g, b, _] = image.get_pixel(px_x, px_y).0;
                    accum_r += r as u32;
                    accum_g += g as u32;
                    accum_b += b as u32;
                }
            }

            let divisor = width_chunk_size * height_chunk_size;
            let final_average_for_chunk = Rgb([(accum_r / divisor) as u8, (accum_g / divisor) as u8, (accum_b / divisor) as u8]);
            av_px_colours.push(final_average_for_chunk);
        }
    }

    av_px_colours
}

fn convert_to_palette (input: &DynamicImage, palette: Vec<Rgb<u8>>) -> DynamicImage {
    let mut new_img = DynamicImage::new(input.width(), input.height(), ColorType::Rgb8);

    for x in 0..input.width() {
        if x % 100 == 0 {
            println!("\tprocessing col {}", x + 1);
        }

        for y in 0..input.height() {
            let [r, g, b, _] = input.get_pixel(x, y).0;

            let chosen_new_colour = palette.iter().copied().min_by_key(|rgb| {
                let [cmp_r, cmp_g, cmp_b] = rgb.0;

                let delta_r = r.max(cmp_r) - r.min(cmp_r);
                let delta_g = g.max(cmp_g) - g.min(cmp_g);
                let delta_b = b.max(cmp_b) - b.min(cmp_b);

                (delta_r as u32).pow(2) + (delta_g as u32).pow(2) + (delta_b as u32).pow(2)
            }).unwrap();

            new_img.put_pixel(x, y, chosen_new_colour.to_rgba());
        }
    }


    new_img
}