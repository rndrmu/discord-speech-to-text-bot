mod stt;
mod ehandler;

use std::{path::PathBuf, fmt, io::Cursor};
use tokio::fs::File;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};
use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use poise::serenity_prelude::{self as serenity, Mutex};
use tokio::io::AsyncReadExt;
use hound;

use stt::*;

/// Initial, fast model
pub static WHISPER_CTX: Lazy<Mutex<WhisperContext>> =
    Lazy::new(|| Mutex::new(WhisperContext::new("./ggml-small.bin").unwrap()));

/// More accurate model (but slower) - for post processing on demand
pub static WHISPER_POST_PROCESS_CTX: Lazy<Mutex<WhisperContext>> =
    Lazy::new(|| Mutex::new(WhisperContext::new("./ggml-medium.bin").unwrap()));

#[derive(Debug)]
pub enum Error {
    Serenity(serenity::Error),
    Io(std::io::Error),
    Hound(hound::Error),
    TokioJoin(tokio::task::JoinError),
    WithMessage(String),
    Generic(Box<dyn std::error::Error  + Send + Sync>),
    Anyhow(anyhow::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Serenity(e) => write!(f, "Serenity error: {}", e),
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Hound(e) => write!(f, "Hound error: {}", e),
            Error::TokioJoin(e) => write!(f, "Tokio join error: {}", e),
            Error::WithMessage(e) => write!(f, "Error: {}", e),
            Error::Generic(e) => write!(f, "Generic error: {}", e),
            Error::Anyhow(e) => write!(f, "Anyhow error: {}", e),
        }
    }
}

pub struct Data {} // User data, which is stored and accessible in all command invocations
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[poise::command(
    context_menu_command = "Transcribe Voice Message",
)]
pub async fn transcribe(ctx: Context<'_>, msg: serenity::Message) -> Result<(), Error> {
    ctx.defer_or_broadcast().await.map_err(Error::Serenity)?;


    // if attachment is not a voice message, of mime type audio/ogg, return
    let file = match msg.attachments.get(0) {
        Some(v) => v,
        None => {
            return Err(Error::WithMessage("No attachment found".to_string()));
        }
    };


    // if file >8mb reject
    if file.size > 8_000_000 {
        return Err(Error::WithMessage("File too large".to_string()));
    }

    
    let start_time = std::time::Instant::now();


    
    // dl into "./voices/<executor username>-<timestamp>.ogg"
    let fname = format!("./voices/{}-{}.ogg", msg.author.id.0, msg.timestamp.timestamp());
    let out = fname.replace(".ogg", ".wav");

    if std::fs::metadata(fname.clone()).is_err() {
        fetch_url(file.url.clone(), fname.clone()).await.map_err(Error::Anyhow)?;
    }

    transcode_video(&fname, &out).await.unwrap();
    let stt_res = speech_to_text(out.clone()).await;

    // if transcript exceeds 2000 characters, split it into multiple messages
    println!("Transcript: {:?}", stt_res);

    let transcript = stt_res.unwrap_or_else(|_| "Failed to transcribe message".to_string());

    ctx.send(|m| {
        m.embed(|e| {
            e.title("Transcript");
            e.description(transcript);
            e.colour(serenity::Colour::from_rgb(0, 255, 0));
            e.footer(|f| {
                f.text(format!(
                    "Took {}s · Powered by Rust & OpenAI · Invite me! -> https://waa.ai/transbot",
                     start_time.elapsed().as_secs_f32()));
                f
            })
        })
    }).await.map_err(Error::Serenity)?;
    
    // delete files
    std::fs::remove_file(fname).unwrap();
    std::fs::remove_file(out).unwrap();

    Ok(())
}


/// register command
#[poise::command(
    prefix_command,
    owners_only,
)]
async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await.map_err(Error::Serenity)?;
    Ok(())
}



#[tokio::main]
async fn main() {
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                    transcribe(), 
                    register(),
                    privacy(),
                    tos(),
                    invite(),
                    help(),
                ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".to_string()),
                ..Default::default()
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(
                    async move { ehandler::event_listener(ctx, event, &framework, data).await },
                )
            },
            ..Default::default()
        })
        .token(std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN"))
        .intents(serenity::GatewayIntents::GUILD_MESSAGES | serenity::GatewayIntents::MESSAGE_CONTENT )
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                Ok(Data {})
            })
        });

    framework.run().await.unwrap();
}

/// View the privacy policy of the bot
#[poise::command(
    slash_command
)]
pub async fn privacy(ctx: Context<'_>) -> Result<(), Error> {
    // read "privacy.md" and send it
    let mut file = File::open("privacy.md").await.map_err(Error::Io)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.map_err(Error::Io)?;

    ctx.send(|m| {
        m.embed(|e| {
            e.title("Privacy Policy");
            e.description(contents);
            e
        });
        m
    }).await.map_err(Error::Serenity)?;


    Ok(())
}

/// View the TOS of the bot
#[poise::command(
    slash_command
)]
pub async fn tos(ctx: Context<'_>) -> Result<(), Error> {
    // read "tos.md" and send it
    let mut file = File::open("terms.md").await.map_err(Error::Io)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.map_err(Error::Io)?;

    ctx.send(|m| {
        m.embed(|e| {
            e.title("Terms of Service");
            e.description(contents);
            e
        });
        m
    }).await.map_err(Error::Serenity)?;

    Ok(())
}

/// Return bot's invite link
#[poise::command(
    slash_command
)]
pub async fn invite(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(|m| {
        m.content("Invite me to your server! <https://discord.com/oauth2/authorize?client_id=1097088747281072198&permissions=3072&scope=bot> or click the button below")
        .components(|c| {
            c.create_action_row(|r| {
                r.create_button(|b| {
                    b.label("Invite me!")
                    .url("https://discord.com/oauth2/authorize?client_id=1097088747281072198&permissions=3072&scope=bot")
                    .style(serenity::ButtonStyle::Link);
                    b
                });
                r
            });
            c
        });
        m
    }).await.map_err(Error::Serenity)?;

    Ok(())
}


/// help command
#[poise::command(
    slash_command,
    prefix_command,
    aliases("h"),
    track_edits,
)]
pub async fn help(
    ctx: Context<'_>,
) -> Result<(), Error> {

    let mut embed = serenity::builder::CreateEmbed::default();
    embed.colour(serenity::Colour::from_rgb(0, 255, 0));
    embed.title("Help");
    embed.description("
    Transbot is a bot that transcribes voice messages into text. 
    To use it, simply send a voice message and the bot will automatically transcribe it.
    Alternatively, you can right click on a message > Apps > Transcribe Voice Message.
    It uses OpenAI's Whisper API to generate the transcript. 
    It is still in beta, so please report any bugs you find to the support server. 
    You can also suggest features to add to the bot. 
    The bot is open source, so you can contribute to it too! 
    The source code is available at https://github.com/rndrmu/discord-speech-to-text-bot
    ");
    embed.field("Commands", "
    `/help` - View this message
    `/privacy` - View the privacy policy of the bot
    `/tos` - View the terms of service of the bot
    `/invite` - Get the bot's invite link
    ", false);
    embed.field("Support", "https://discord.gg/rFUQFHnXYT", false);

    ctx.send(|m| {
        m.embed(|e| {
            e.0 = embed.0;
            e
        });
        m
    }).await.map_err(Error::Serenity)?;

    Ok(())
}