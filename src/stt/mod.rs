mod transcode;



use std::{path::PathBuf, fmt, io::Cursor};
use tokio::fs::File;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};
use poise::serenity_prelude::{self as serenity, Mutex};
use anyhow::{Result, anyhow};

use crate::WHISPER_CTX;

fn attachment_is_audio(attachment: serenity::Attachment) -> bool {
    attachment.content_type.unwrap_or_else(|| 
        // if content type is not set, use an empty string so the "starts_with" call returns false
        "".to_string()
    ).starts_with("audio/")
}


pub async fn transcode_video(nin: &str, out: &str) -> Result<()> {
    let _res = tokio::process::Command::new("ffmpeg")
        .arg("-loglevel")
        .arg("quiet")
        .arg("-y")
        .arg("-i")
        .arg(nin)
        .arg("-ar")
        .arg("16000")
        .arg(out)
        .status()
        .await?;
    Ok(())
}

pub async fn speech_to_text(file: String) -> Result<String> {

    let mut ctx = WHISPER_CTX.lock().await;


    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
    /* params.set_translate(true);
    params.set_language(Some("en")); */
    //logs
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);



    let file_path = PathBuf::from(file.clone());
    let res = tokio::task::spawn_blocking(move || -> Result<String> {
        ctx.full(params, &wav_to_integer_mono(&file_path)?)
            .map_err(|x| anyhow!(format!("{x:?}")))?;

        let num_segments = ctx.full_n_segments();
        let res = (0..num_segments)
            .flat_map(|i| ctx.full_get_segment_text(i).map_err(|x| anyhow!(format!("{x:?}"))))
            .collect::<Vec<String>>()
            .join("\n");
        Ok(res)
    })
    .await??;

    Ok(res)
}

pub fn wav_to_integer_mono(file: &PathBuf) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(file)?;
    let hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: _,
        ..
    } = reader.spec();
    let r = &reader
        .samples::<i16>()
        .map(|s| s.expect("invalid sample"))
        .collect::<Vec<_>>();
    let mut audio = whisper_rs::convert_integer_to_float_audio(r);

    if sample_rate != 16000 {
        return Err(anyhow!("Sample Rate Issue!"));
    }

    if channels == 2 {
        audio = whisper_rs::convert_stereo_to_mono_audio(&audio).unwrap();
    }

    Ok(audio)
}

pub async fn fetch_url(url: String, file_name: String) -> Result<()> {
    let response = reqwest::get(url).await?;
    let mut file = std::fs::File::create(file_name)?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;
    Ok(())
}