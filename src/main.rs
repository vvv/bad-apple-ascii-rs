use std::{thread, time::Duration};

use color_eyre::eyre::{self, eyre};
use ffmpeg_next::{self as ffmpeg, format::Pixel, frame::Video, software::scaling::Flags};
use img_to_ascii::image::LumaImage;
use terminal_size::{Width, terminal_size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum FrameRate {
    Fps30,
    Fps60,
}

impl FrameRate {
    fn period(&self) -> Duration {
        let fps: u8 = match self {
            Self::Fps30 => 30,
            Self::Fps60 => 60,
        };
        Duration::from_secs_f32(f32::from(fps).recip())
    }
}

fn extract_frames(path: &str, frame_rate: FrameRate) -> eyre::Result<Vec<image::DynamicImage>> {
    ffmpeg::init()?;
    ffmpeg::log::set_level(ffmpeg::log::Level::Error); // suppress Info and Warn

    let mut ictx = ffmpeg::format::input(&path)?;
    let input = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| eyre!("No video stream"))?;

    let context_decoder = ffmpeg_next::codec::Context::from_parameters(input.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        // We don't convert to `Pixel::GRAY8` because `image-to-ascii` expects RGB input
        Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )?;

    let mut frame_idx = 0;
    let mut frames = Vec::new();

    let video_stream_index = input.index();
    for (stream, packet) in ictx.packets() {
        if stream.index() != video_stream_index {
            continue;
        }
        decoder.send_packet(&packet)?;

        let mut decoded = Video::empty();

        while decoder.receive_frame(&mut decoded).is_ok() {
            frame_idx += 1;
            if frame_rate == FrameRate::Fps30 && frame_idx % 2 == 1 {
                continue;
            }
            let mut frame = Video::empty();
            scaler.run(&decoded, &mut frame)?;

            let rgb_image =
                image::RgbImage::from_vec(frame.width(), frame.height(), frame.data(0).to_vec())
                    .ok_or_else(|| eyre!("Failed to create RgbImage"))?;
            frames.push(rgb_image.into());
        }
    }
    decoder.send_eof()?;

    Ok(frames)
}

fn main() -> eyre::Result<()> {
    let font = include_bytes!("../fonts/bitocra-13.bdf");
    let alphabet = include_str!("../alphabets/alphabet.txt")
        .chars()
        .collect::<Vec<_>>();

    let invert = false;
    let font = img_to_ascii::font::Font::from_bdf_stream(font.as_ref(), &alphabet, invert);

    let (Width(max_width), _height) = terminal_size().expect("BUG");
    let width = Some(max_width.into());
    let brightness_offset = 0.;
    let brightness_scale = 0.25;
    let edge_brightness_scale = 1.;

    let frame_rate = FrameRate::Fps30;
    for frame in extract_frames("bad-apple.mp4", frame_rate)? {
        let ascii = img_to_ascii::convert::img_to_char_rows(
            &font,
            &LumaImage::naive_grayscale_from(&frame),
            img_to_ascii::convert::direction_and_intensity_convert,
            width,
            brightness_offset / 255.,
            brightness_scale,
            edge_brightness_scale,
            &img_to_ascii::convert::ConversionAlgorithm::EdgeAugmented,
        );
        assert!(ascii.iter().all(|row| row.len() == ascii[0].len()));

        let img_ascii =
            img_to_ascii::convert::char_rows_to_color_bitmap(&ascii, &font, &frame, invert);
        img_ascii.save("/tmp/1.png")?;

        print!("\x1b[2J\x1b[H"); // clear the terminal
        for row in &ascii {
            for &ch in row {
                print!("{ch}");
            }
            println!();
        }

        thread::sleep(frame_rate.period());
    }

    Ok(())
}
