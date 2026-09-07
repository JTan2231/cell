mod careers;
mod discovery;

pub use careers::verify;
pub use discovery::{discover, prepare_query};

/// Returns a canonical board URL only for an explicitly supported ATS host.
#[must_use]
pub fn canonical_board_url(input: &str) -> Option<String> {
    Board::from_url(&Url::parse(input).ok()?).map(|board| board.url())
}

/// Identifies an ATS tenant independently from job IDs and shared hosting domains.
#[must_use]
pub fn board_identity(input: &str) -> Option<String> {
    Board::from_url(&Url::parse(input).ok()?).map(|board| board.identity())
}

/// Recognizes known third-party discovery surfaces, whose host is not job ownership.
#[must_use]
pub fn is_discovery_directory(input: &str) -> bool {
    Url::parse(input)
        .ok()
        .and_then(|url| url.host_str().map(directory_host))
        .unwrap_or(false)
}

/// Filters freeform hiring-thread headers into candidate display names.
#[must_use]
pub fn plausible_company_name(name: &str) -> bool {
    let name = name.trim();
    let lower = name.to_lowercase();
    !name.is_empty()
        && name.chars().count() <= 80
        && name.split_whitespace().count() <= 12
        && ![
            "we ", "we're ", "we’ve ", "we've ", "i ", "i've ", "i’m ", "i'm ", "hi ", "hello ",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn unsupported_shared_ats(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        ["join.com", "curriculo.me"]
            .iter()
            .any(|shared| host == *shared || host.ends_with(&format!(".{shared}")))
    })
}

use crate::{http::public_url, models::Evidence};
use reqwest::Url;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::BTreeSet;

fn string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn id(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|v| {
        v.as_str()
            .map(ToOwned::to_owned)
            .or_else(|| v.as_u64().map(|id| id.to_string()))
    })
}

fn evidence(url: &str, kind: &str, note: impl Into<String>) -> Evidence {
    Evidence {
        source_url: url.into(),
        kind: kind.into(),
        note: note.into(),
        parser_version: Some("cast-adapters-v3".into()),
        ..Default::default()
    }
}

fn text(html: &str) -> String {
    Html::parse_fragment(html)
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn links(html: &str, base: &Url) -> Vec<(Url, String)> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a[href], iframe[src]")
        .unwrap_or_else(|_| unreachable!("constant selector"));
    let mut seen = BTreeSet::new();
    document
        .select(&selector)
        .filter_map(|element| {
            let raw = element
                .value()
                .attr("href")
                .or_else(|| element.value().attr("src"))?;
            let mut url = base.join(raw).ok()?;
            url.set_fragment(None);
            public_url(url.as_str()).ok()?;
            if !seen.insert(url.as_str().to_owned()) {
                return None;
            }
            Some((url, element.text().collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Board {
    Greenhouse(String),
    Ashby(String),
    Lever { site: String, eu: bool },
}

impl Board {
    fn from_url(url: &Url) -> Option<Self> {
        let path: Vec<_> = url
            .path_segments()?
            .filter(|part| !part.is_empty())
            .collect();
        let token = |index: usize| -> Option<String> {
            path.get(index)
                .filter(|part| {
                    !part.is_empty()
                        && part.len() <= 128
                        && part
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                })
                .map(|part| (*part).to_owned())
        };
        match url.host_str()? {
            "boards.greenhouse.io" | "job-boards.greenhouse.io" => {
                if path.first() == Some(&"embed") {
                    url.query_pairs()
                        .find(|(name, _)| name == "for")
                        .and_then(|(_, value)| {
                            let board = value.into_owned();
                            if board.len() <= 128
                                && !board.is_empty()
                                && board
                                    .chars()
                                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                            {
                                Some(Self::Greenhouse(board))
                            } else {
                                None
                            }
                        })
                } else {
                    token(0).map(Self::Greenhouse)
                }
            }
            "boards-api.greenhouse.io"
                if path.first() == Some(&"v1") && path.get(1) == Some(&"boards") =>
            {
                token(2).map(Self::Greenhouse)
            }
            "jobs.ashbyhq.com" => token(0).map(Self::Ashby),
            "api.ashbyhq.com"
                if path.first() == Some(&"posting-api") && path.get(1) == Some(&"job-board") =>
            {
                token(2).map(Self::Ashby)
            }
            "jobs.lever.co" | "jobs.eu.lever.co" => token(0).map(|site| Self::Lever {
                site,
                eu: url.host_str() == Some("jobs.eu.lever.co"),
            }),
            "api.lever.co" | "api.eu.lever.co"
                if path.first() == Some(&"v0") && path.get(1) == Some(&"postings") =>
            {
                token(2).map(|site| Self::Lever {
                    site,
                    eu: url.host_str() == Some("api.eu.lever.co"),
                })
            }
            _ => None,
        }
    }

    fn url(&self) -> String {
        match self {
            Self::Greenhouse(board) => format!("https://job-boards.greenhouse.io/{board}"),
            Self::Ashby(board) => format!("https://jobs.ashbyhq.com/{board}"),
            Self::Lever { site, eu } => format!(
                "https://jobs.{}lever.co/{site}",
                if *eu { "eu." } else { "" }
            ),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::Greenhouse(board) => format!("greenhouse:{board}"),
            Self::Ashby(board) => format!("ashby:{board}"),
            Self::Lever { site, eu } => {
                format!("lever:{}:{site}", if *eu { "eu" } else { "global" })
            }
        }
    }
}

fn directory_host(host: &str) -> bool {
    [
        "ycombinator.com",
        "news.ycombinator.com",
        "linkedin.com",
        "github.com",
        "wellfound.com",
        "indeed.com",
        "glassdoor.com",
        "ziprecruiter.com",
        "simplyhired.com",
        "monster.com",
        "naukri.com",
        "totaljobs.com",
        "ambitionbox.com",
        "velvetjobs.com",
        "web3.career",
        "remoterocketship.com",
        "trueup.io",
        "jobright.ai",
        "fastaijobs.com",
        "6figr.com",
        "careerexplorer.com",
        "join.com",
        "curriculo.me",
        "builtin.com",
        "builtinnyc.com",
        "builtinchicago.org",
        "builtincolorado.com",
        "builtinaustin.com",
        "builtinsf.com",
        "builtinla.com",
        "builtinboston.com",
        "builtinseattle.com",
        "a16z.com",
        "jobs.a16z.com",
        "google.com",
        "forms.gle",
        "youtube.com",
        "twitter.com",
        "x.com",
        "reddit.com",
        "medium.com",
        "substack.com",
        "notion.site",
        "notion.so",
        "docs.google.com",
        "bit.ly",
        "t.co",
    ]
    .iter()
    .any(|item| host == *item || host.ends_with(&format!(".{item}")))
}

fn employer_domain(url: &Url) -> Option<String> {
    let host = url
        .host_str()?
        .strip_prefix("www.")
        .unwrap_or(url.host_str()?);
    if Board::from_url(url).is_some()
        || directory_host(host)
        || [
            "greenhouse.io",
            "ashbyhq.com",
            "lever.co",
            "workable.com",
            "smartrecruiters.com",
            "myworkdayjobs.com",
            "bamboohr.com",
            "personio.com",
            "recruitee.com",
            "teamtailor.com",
        ]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return None;
    }
    Some(host.into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn board_identity_keeps_hosted_tenants_separate() {
        let a =
            Board::from_url(&Url::parse("https://jobs.ashbyhq.com/acme/a-job").unwrap()).unwrap();
        let b =
            Board::from_url(&Url::parse("https://jobs.ashbyhq.com/other/a-job").unwrap()).unwrap();
        assert_ne!(a.identity(), b.identity());
        assert!(employer_domain(&Url::parse(&a.url()).unwrap()).is_none());
        assert_ne!(
            Board::from_url(&Url::parse("https://jobs.eu.lever.co/acme/123").unwrap())
                .unwrap()
                .identity(),
            Board::from_url(&Url::parse("https://jobs.lever.co/acme/123").unwrap())
                .unwrap()
                .identity()
        );
    }
}
