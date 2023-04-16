use crate::{
    Data, Error, stt::{transcode_video, speech_to_text, fetch_url}
};
use poise::serenity_prelude::{self as serenity, AttachmentType, Mentionable};

pub async fn event_listener(
    ctx: &serenity::Context,
    event: &poise::Event<'_>,
    fw: &poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        poise::Event::Message { new_message } => {
            let file = match new_message.attachments.get(0) {
                Some(v) => v,
                None => {
                    return Ok(()) // quit silently
                }
            };

            // if file >8mb reject
            if file.size > 8_000_000 {
                return Ok(())
            }
        
            let start_time = std::time::Instant::now();
            let fname = format!("./voices/{}-{}.ogg", new_message.author.id.0, new_message.timestamp.timestamp());
            let out = fname.replace(".ogg", ".wav");
        
            if std::fs::metadata(fname.clone()).is_err() {
                fetch_url(file.url.clone(), fname.clone()).await.map_err(Error::Anyhow)?;
            }
        
            transcode_video(&fname, &out).await.unwrap();
            let stt_res = speech_to_text(out.clone()).await;
        
            // if transcript exceeds 2000 characters, split it into multiple messages
        
            let transcript = stt_res.unwrap_or_else(|_| "Failed to transcribe message".to_string());
        
            new_message.channel_id
                .send_message(&ctx, |f| {
                    f.embed(|e| {
                        e.title("Voice Message Transcript");
                        e.description(transcript);
                        e.colour(serenity::Colour::from_rgb(0, 255, 0));
                        e.footer(|f| {
                            f.text(format!(
                                "Took {}s · Powered by Rust & OpenAI · Invite me! -> https://waa.ai/transbot",
                                 start_time.elapsed().as_secs_f32()));
                            f
                        })
                    })
                    .reference_message(new_message)
                    .allowed_mentions(|m| m.replied_user(false))
                }).await.map_err(Error::Serenity)?;
            
            // delete files
            std::fs::remove_file(fname).unwrap();
            std::fs::remove_file(out).unwrap();

        }
        _ => {}
    }

    Ok(())
}
