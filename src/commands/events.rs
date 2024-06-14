use std::borrow::Cow;
use std::ops::Add;

use anyhow::Context as _;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use ics::properties::{Description, DtEnd, DtStart, Location, Summary};
use ics::{Event, ICalendar};
use image::EncodableLayout;
use poise::serenity_prelude::{
    CreateAttachment, CreateScheduledEvent, CreateThread, ReactionType, ScheduledEventType,
};

use crate::constants::TIMEZONE;
use crate::{done, Context, Error};

const EVENT_URL: &str = "https://discord.com/events/";

/// Export all events on this server as ICS calendar file
#[poise::command(slash_command, prefix_command, guild_only)]
pub(crate) async fn export_events(ctx: Context<'_>) -> Result<(), Error> {
    const ICS_TIME_FORMAT: &str = "%Y%m%dT%H%M%SZ";

    ctx.defer().await?;
    let events = ctx
        .guild_id()
        .expect("guild_only")
        .scheduled_events(ctx.http(), false)
        .await?;
    let mut calendar = ICalendar::new("2.0", "ics-rs");
    for event in events {
        let mut ics_event = Event::new(
            event.id.get().to_string(),
            Utc::now().format(ICS_TIME_FORMAT).to_string(),
        );

        ics_event.push(Summary::new(event.name));
        if let Some(description) = event.description {
            ics_event.push(Description::new(description));
        }
        if let Some(metadata) = event.metadata {
            if let Some(loc) = metadata.location {
                ics_event.push(Location::new(loc));
            }
        }
        ics_event.push(DtStart::new(
            event.start_time.format(ICS_TIME_FORMAT).to_string(),
        ));
        ics_event.push(DtEnd::new(
            event
                .end_time
                .unwrap_or(event.start_time.add(Duration::hours(1)).into())
                .format(ICS_TIME_FORMAT)
                .to_string(),
        ));

        calendar.add_event(ics_event);
    }
    let mut bytes = Vec::new();
    calendar.write(&mut bytes)?;
    ctx.send(CreateReply::default().attachment(CreateAttachment::bytes(
        Cow::from(bytes.as_bytes()),
        "calendar.ics".to_string(),
    )))
    .await?;
    done!(ctx);
}

/// Create a new meetup
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    required_permissions = "MANAGE_EVENTS"
)]
pub(crate) async fn event(
    ctx: Context<'_>,
    name: String,
    location: String,
    #[description = "yyyy-mm-dd hh:mm, example: 2012-12-21 12:34"] start: String,
    #[description = "yyyy-mm-dd hh:mm, default start_time + 1h"] end: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let (start_date, end_date) = parse_start_and_end_date(start, end)?;
    let guild_id = ctx.guild_id().expect("guild_only");
    let event = guild_id
        .create_scheduled_event(
            ctx.http(),
            CreateScheduledEvent::new(ScheduledEventType::External, &name, start_date)
                .location(location)
                .end_time(end_date),
        )
        .await?;
    let announcement = format!(
        "[{}]({}{}/{}) mit {}",
        name,
        EVENT_URL,
        event.guild_id,
        event.id,
        ctx.author()
    );
    let announcement_channel = ctx
        .data()
        .event_channel_per_guild
        .get(&guild_id)
        .with_context(|| format!("No announcement channel configured for guild {guild_id}"))?;

    let msg = announcement_channel.say(ctx.http(), announcement).await?;
    msg.react(ctx.http(), ReactionType::from('👍')).await?;
    msg.react(ctx.http(), ReactionType::from('❔')).await?;

    let thread = announcement_channel
        .create_thread_from_message(ctx.http(), msg.id, CreateThread::new(name))
        .await?;
    thread.id.add_thread_member(ctx, ctx.author().id).await?;
    done!(ctx);
}

fn parse_start_and_end_date(
    start: String,
    end: Option<String>,
) -> Result<(DateTime<Tz>, DateTime<Tz>), Error> {
    const TIME_FORMAT: &'static str = "%Y-%m-%d %H:%M";

    let start_date = NaiveDateTime::parse_from_str(&start, TIME_FORMAT)
        .map(|date| date.and_local_timezone(TIMEZONE).unwrap())
        .context("Couldn't parse start time")?;
    let end_date = match end {
        None => start_date.add(Duration::hours(1)),
        Some(input) => NaiveDateTime::parse_from_str(&input, TIME_FORMAT)
            .map(|date| date.and_local_timezone(TIMEZONE).unwrap())
            .context("Couldn't parse end time")?,
    };
    Ok((start_date, end_date))
}

#[cfg(test)]
mod tests {
    use super::parse_start_and_end_date;

    #[test]
    fn test_parse_dates() {
        let (start, end) = parse_start_and_end_date("1970-01-01 00:00".to_string(), None).unwrap();
        assert_eq!(start.to_rfc3339(), "1970-01-01T00:00:00+01:00");
        assert_eq!(end.to_rfc3339(), "1970-01-01T01:00:00+01:00");

        let (start, end) = parse_start_and_end_date(
            "2012-12-31 12:34".to_string(),
            Some("2013-01-01 21:43".to_string()),
        )
        .unwrap();
        assert_eq!(start.to_rfc3339(), "2012-12-31T12:34:00+01:00");
        assert_eq!(end.to_rfc3339(), "2013-01-01T21:43:00+01:00");
    }

    #[test]
    fn test_reject_invalid_dates() {
        assert!(parse_start_and_end_date("".to_string(), None).is_err());
        assert!(
            parse_start_and_end_date("1970-01-01 00:00".to_string(), Some("".to_string())).is_err()
        );
        assert!(parse_start_and_end_date("not a date".to_string(), None).is_err());
        assert!(parse_start_and_end_date(
            "1970-01-01 00:00".to_string(),
            Some("not a date".to_string()),
        )
        .is_err());
    }
}
