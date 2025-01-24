use std::collections::HashMap;
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


    convert_to_palette(&image, &av_px_colours, euclidean_distance)?;
    convert_to_palette(&image, &av_px_colours, manhattan_distance)?;
    convert_to_palette(&image, &av_px_colours, sum_diff)?;
    convert_to_palette(&image, &av_px_colours, prod_diff)?;

    Ok(())
}


fn get_av_px_colours (image: &DynamicImage) -> Vec<Rgb<u8>> {
    const LENGTH_SECTION_SIZE: u32 = 4;
    let (width, height) = image.dimensions();
    let (width_chunk_size, height_chunk_size) = (width / LENGTH_SECTION_SIZE, height / LENGTH_SECTION_SIZE);

    let mut av_px_colours = Vec::with_capacity((LENGTH_SECTION_SIZE * LENGTH_SECTION_SIZE) as usize);
    for chunk_x in 0..LENGTH_SECTION_SIZE {
        println!("\tprocessing x chunk {}", chunk_x + 1);
        for chunk_y in 0..LENGTH_SECTION_SIZE {

            let mut map: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y).to_rgb();
                    const THRESHOLD: u32 = 100;
                    let too_close = av_px_colours.iter().map(|cmp_px| euclidean_distance(&px, cmp_px)).min().map_or(false, |x| x < THRESHOLD);

                    if !too_close {
                        *map.entry(px).or_default() += 1;
                    }
                }
            }

            let (most_common, _) = map.into_iter().max_by_key(|(_, count)| *count).unwrap();
            av_px_colours.push(most_common);
        }
    }

    av_px_colours
}

fn convert_to_palette (input: &DynamicImage, palette: &[Rgb<u8>], dist_func: impl Fn(&Rgb<u8>, &Rgb<u8>) -> u32) -> anyhow::Result<()> {
    let mut new_img = DynamicImage::new(input.width(), input.height(), ColorType::Rgb8);

    let fn_name = std::any::type_name_of_val(&dist_func);
    println!("starting to convert with {fn_name}");
    for x in 0..input.width() {
        if x % 100 == 0 {
            println!("\tprocessing col {}", x + 1);
        }

        for y in 0..input.height() {
            let px = input.get_pixel(x, y).to_rgb();

            let chosen_new_colour = palette.iter().copied().min_by_key(|rgb| {
                dist_func(rgb, &px)
            }).unwrap();

            new_img.put_pixel(x, y, chosen_new_colour.to_rgba());
        }
    }

    println!("finished conversion, saving");
    new_img.save(format!("{fn_name}.jpg"))?;
    Ok(())
}

#[inline]
fn euclidean_distance(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let delta_r = r.max(cmp_r) - r.min(cmp_r);
    let delta_g = g.max(cmp_g) - g.min(cmp_g);
    let delta_b = b.max(cmp_b) - b.min(cmp_b);

    (delta_r as u32).pow(2) + (delta_g as u32).pow(2) + (delta_b as u32).pow(2)
}

#[inline]
fn manhattan_distance(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let delta_r = r.max(cmp_r) - r.min(cmp_r);
    let delta_g = g.max(cmp_g) - g.min(cmp_g);
    let delta_b = b.max(cmp_b) - b.min(cmp_b);

    delta_r as u32 + delta_g as u32 + delta_b as u32
}

#[inline]
fn sum_diff(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let a = *r as u32 + *g as u32 + *b as u32;
    let b = *cmp_r as u32 + *cmp_g as u32 + *cmp_b as u32;

    a.max(b) - a.min(b)
}

#[inline]
fn prod_diff(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let a = *r as u32 * *g as u32 * *b as u32;
    let b = *cmp_r as u32 * *cmp_g as u32 * *cmp_b as u32;

    a.max(b) - a.min(b)
}
