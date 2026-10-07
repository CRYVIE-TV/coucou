// GitHub pulse and contribution calendar. The parse rules and the alert rules
// match GitHubPulse.swift and GitHubActivity.swift: the first poll is silent,
// a new commit can alert immediately, and a green default branch never does.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

pub const PULSE_QUERY: &str = r#"
query {
  viewer {
    login
    pullRequests(states: OPEN, first: 20, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number title url isDraft reviewDecision
        repository { nameWithOwner url }
        commits(last: 1) {
          nodes { commit { oid statusCheckRollup { state } } }
        }
      }
    }
    repositories(first: 10, ownerAffiliations: [OWNER], orderBy: {field: PUSHED_AT, direction: DESC}) {
      nodes {
        nameWithOwner url isArchived
        defaultBranchRef {
          name
          target { ... on Commit { oid statusCheckRollup { state } } }
        }
      }
    }
  }
  reviewRequested: search(query: "is:pr is:open review-requested:@me archived:false", type: ISSUE, first: 20) {
    issueCount
    nodes {
      ... on PullRequest {
        number title url isDraft
        author { login }
        repository { nameWithOwner url }
      }
    }
  }
}
"#;

pub const ACTIVITY_QUERY: &str = r#"
query {
  viewer {
    login
    contributionsCollection {
      contributionCalendar {
        totalContributions
        weeks {
          contributionDays {
            date contributionCount contributionLevel weekday
          }
        }
      }
    }
  }
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ci {
    Pending,
    Success,
    Failure,
    Unknown,
}

impl Ci {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.to_ascii_uppercase()).as_deref() {
            Some("PENDING") | Some("EXPECTED") => Self::Pending,
            Some("SUCCESS") => Self::Success,
            Some("ERROR") | Some("FAILURE") => Self::Failure,
            _ => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Review {
    Approved,
    ChangesRequested,
    Pending,
    Unknown,
}

impl Review {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.to_ascii_uppercase()).as_deref() {
            Some("APPROVED") => Self::Approved,
            Some("CHANGES_REQUESTED") => Self::ChangesRequested,
            Some("REVIEW_REQUIRED") => Self::Pending,
            _ => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changesRequested",
            Self::Pending => "pending",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PullRequest {
    pub id: String,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub number: i64,
    pub is_draft: bool,
    pub ci: Ci,
    pub review: Review,
    pub head_sha: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepoCi {
    pub repo: String,
    pub url: String,
    pub branch: String,
    pub ci: Ci,
    pub head_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pulse {
    pub login: String,
    pub my_prs: Vec<PullRequest>,
    pub to_review: Vec<PullRequest>,
    pub main_ci: Vec<RepoCi>,
}

impl Pulse {
    pub fn has_pending(&self) -> bool {
        self.my_prs.iter().any(|pr| pr.ci == Ci::Pending)
            || self.main_ci.iter().any(|repo| repo.ci == Ci::Pending)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhEvent {
    CiFailed { pr_id: String },
    CiPassed { pr_id: String },
    MainFailed { repo: String },
    ReviewRequested { pr_id: String },
}

/// One alert per poll. Failure outranks a review request, which outranks a green PR.
pub struct Alert {
    pub success: bool,
    pub label: String,
    pub sound: &'static str,
}

pub fn primary_alert(events: &[GhEvent]) -> Option<Alert> {
    let mut best: Option<(u8, Alert)> = None;
    for event in events {
        let (rank, alert) = match event {
            GhEvent::CiFailed { pr_id } => (
                3,
                Alert {
                    success: false,
                    label: format!("CI failed · {pr_id}"),
                    sound: "error",
                },
            ),
            GhEvent::MainFailed { repo } => (
                3,
                Alert {
                    success: false,
                    label: format!("CI failed · {repo}"),
                    sound: "error",
                },
            ),
            GhEvent::ReviewRequested { pr_id } => (
                2,
                Alert {
                    success: true,
                    label: format!("Review · {pr_id}"),
                    sound: "question",
                },
            ),
            GhEvent::CiPassed { pr_id } => (
                1,
                Alert {
                    success: true,
                    label: format!("CI passed · {pr_id}"),
                    sound: "finish",
                },
            ),
        };
        if best.as_ref().map(|(have, _)| rank > *have).unwrap_or(true) {
            best = Some((rank, alert));
        }
    }
    best.map(|(_, alert)| alert)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Day {
    pub date: String,
    pub count: i64,
    pub level: u8,
    pub weekday: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub total: i64,
    pub weeks: Vec<Vec<Day>>,
}

impl Activity {
    #[cfg(test)]
    pub fn last_weeks(&self, n: usize) -> &[Vec<Day>] {
        if n == 0 || self.weeks.is_empty() {
            return &[];
        }
        let start = self.weeks.len().saturating_sub(n);
        &self.weeks[start..]
    }

    #[cfg(test)]
    pub fn last_days(&self, n: usize) -> Vec<&Day> {
        if n == 0 {
            return Vec::new();
        }
        let all: Vec<&Day> = self.weeks.iter().flatten().collect();
        let start = all.len().saturating_sub(n);
        all[start..].to_vec()
    }
}

#[cfg(test)]
pub fn is_stale(fetched_at_ms: Option<u64>, now_ms: u64, max_age_ms: u64) -> bool {
    match fetched_at_ms {
        None => true,
        Some(t) => now_ms.saturating_sub(t) > max_age_ms,
    }
}

pub fn parse_pulse(root: &Value) -> Option<Pulse> {
    let viewer = root.get("data")?.get("viewer")?;
    let login = viewer.get("login").and_then(Value::as_str).unwrap_or("").to_string();

    let mut my_prs = Vec::new();
    let mut seen_pr = std::collections::HashSet::new();
    if let Some(nodes) = viewer.pointer("/pullRequests/nodes").and_then(Value::as_array) {
        for node in nodes {
            if let Some(pr) = parse_my_pr(node) {
                if seen_pr.insert(pr.id.clone()) {
                    my_prs.push(pr);
                }
            }
        }
    }

    let mut main_ci = Vec::new();
    if let Some(nodes) = viewer.pointer("/repositories/nodes").and_then(Value::as_array) {
        for node in nodes {
            if node.get("isArchived").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            if let Some(repo) = parse_repo_ci(node) {
                main_ci.push(repo);
            }
        }
    }

    let mut to_review = Vec::new();
    let mut seen_review = std::collections::HashSet::new();
    if let Some(nodes) = root.pointer("/data/reviewRequested/nodes").and_then(Value::as_array) {
        for node in nodes {
            if let Some(pr) = parse_review_pr(node) {
                if seen_review.insert(pr.id.clone()) {
                    to_review.push(pr);
                }
            }
        }
    }

    Some(Pulse { login, my_prs, to_review, main_ci })
}

fn text(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty())
}

fn parse_my_pr(node: &Value) -> Option<PullRequest> {
    let number = node.get("number")?.as_i64()?;
    let title = text(node, "title")?;
    let url = text(node, "url")?;
    let repo = node.pointer("/repository/nameWithOwner").and_then(Value::as_str)?;
    let (ci_raw, head_sha) = last_commit(node);
    Some(PullRequest {
        id: format!("{repo}#{number}"),
        title,
        url,
        repo: repo.to_string(),
        number,
        is_draft: node.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        ci: Ci::parse(ci_raw.as_deref()),
        review: Review::parse(node.get("reviewDecision").and_then(Value::as_str)),
        head_sha,
        author: None,
    })
}

fn last_commit(node: &Value) -> (Option<String>, Option<String>) {
    let Some(nodes) = node.pointer("/commits/nodes").and_then(Value::as_array) else {
        return (None, None);
    };
    let Some(commit) = nodes.last().and_then(|n| n.get("commit")) else {
        return (None, None);
    };
    let state = commit
        .pointer("/statusCheckRollup/state")
        .and_then(Value::as_str)
        .map(str::to_string);
    let sha = text(commit, "oid");
    (state, sha)
}

fn parse_repo_ci(node: &Value) -> Option<RepoCi> {
    let repo = text(node, "nameWithOwner")?;
    let url = text(node, "url")?;
    let branch_ref = node.get("defaultBranchRef")?.as_object()?;
    let branch = branch_ref.get("name").and_then(Value::as_str)?;
    let target = branch_ref.get("target");
    let state = target
        .and_then(|t| t.pointer("/statusCheckRollup/state"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let sha = target.and_then(|t| text(t, "oid"));
    Some(RepoCi {
        repo,
        url,
        branch: branch.to_string(),
        ci: Ci::parse(state.as_deref()),
        head_sha: sha,
    })
}

fn parse_review_pr(node: &Value) -> Option<PullRequest> {
    let number = node.get("number")?.as_i64()?;
    let title = text(node, "title")?;
    let url = text(node, "url")?;
    let repo = node.pointer("/repository/nameWithOwner").and_then(Value::as_str)?;
    Some(PullRequest {
        id: format!("{repo}#{number}"),
        title,
        url,
        repo: repo.to_string(),
        number,
        is_draft: node.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        ci: Ci::Unknown,
        review: Review::Pending,
        head_sha: None,
        author: node.pointer("/author/login").and_then(Value::as_str).map(str::to_string),
    })
}

/// `old == None` is the first poll after launch: no alerts.
pub fn events(old: Option<&Pulse>, new: &Pulse) -> Vec<GhEvent> {
    let Some(old) = old else {
        return Vec::new();
    };
    let mut result = Vec::new();

    let mut old_prs: HashMap<&str, &PullRequest> = HashMap::new();
    for pr in &old.my_prs {
        old_prs.entry(pr.id.as_str()).or_insert(pr);
    }
    for pr in &new.my_prs {
        if let Some(prev) = old_prs.get(pr.id.as_str()) {
            if prev.head_sha == pr.head_sha {
                if pr.ci == Ci::Failure && prev.ci != Ci::Failure {
                    result.push(GhEvent::CiFailed { pr_id: pr.id.clone() });
                } else if pr.ci == Ci::Success && prev.ci == Ci::Pending {
                    result.push(GhEvent::CiPassed { pr_id: pr.id.clone() });
                }
            } else if pr.ci == Ci::Success {
                result.push(GhEvent::CiPassed { pr_id: pr.id.clone() });
            } else if pr.ci == Ci::Failure {
                result.push(GhEvent::CiFailed { pr_id: pr.id.clone() });
            }
        } else if pr.ci == Ci::Success {
            result.push(GhEvent::CiPassed { pr_id: pr.id.clone() });
        } else if pr.ci == Ci::Failure {
            result.push(GhEvent::CiFailed { pr_id: pr.id.clone() });
        }
    }

    let mut old_repos: HashMap<&str, &RepoCi> = HashMap::new();
    for repo in &old.main_ci {
        old_repos.entry(repo.repo.as_str()).or_insert(repo);
    }
    for repo in &new.main_ci {
        if let Some(prev) = old_repos.get(repo.repo.as_str()) {
            if prev.head_sha == repo.head_sha {
                if repo.ci == Ci::Failure && prev.ci != Ci::Failure {
                    result.push(GhEvent::MainFailed { repo: repo.repo.clone() });
                }
            } else if repo.ci == Ci::Failure {
                result.push(GhEvent::MainFailed { repo: repo.repo.clone() });
            }
        } else if repo.ci == Ci::Failure {
            result.push(GhEvent::MainFailed { repo: repo.repo.clone() });
        }
    }

    let old_reviews: std::collections::HashSet<&str> =
        old.to_review.iter().map(|pr| pr.id.as_str()).collect();
    for pr in &new.to_review {
        if !old_reviews.contains(pr.id.as_str()) {
            result.push(GhEvent::ReviewRequested { pr_id: pr.id.clone() });
        }
    }
    result
}

pub fn parse_activity(root: &Value) -> Option<Activity> {
    let cal = root.pointer("/data/viewer/contributionsCollection/contributionCalendar")?;
    let total = cal.get("totalContributions")?.as_i64()?;
    let weeks_raw = cal.get("weeks")?.as_array()?;
    let mut weeks = Vec::new();
    for week_raw in weeks_raw {
        let Some(days_raw) = week_raw.get("contributionDays").and_then(Value::as_array) else {
            continue;
        };
        let mut days = Vec::new();
        for day_raw in days_raw {
            let Some(date) = text(day_raw, "date") else { continue };
            let Some(count) = day_raw.get("contributionCount").and_then(Value::as_i64) else { continue };
            let Some(level_str) = day_raw.get("contributionLevel").and_then(Value::as_str) else { continue };
            let Some(weekday) = day_raw.get("weekday").and_then(Value::as_i64) else { continue };
            let level = match level_str {
                "FIRST_QUARTILE" => 1,
                "SECOND_QUARTILE" => 2,
                "THIRD_QUARTILE" => 3,
                "FOURTH_QUARTILE" => 4,
                _ => 0,
            };
            days.push(Day { date, count, level, weekday });
        }
        if !days.is_empty() {
            weeks.push(days);
        }
    }
    Some(Activity { total, weeks })
}

fn pr_json(pr: &PullRequest) -> Value {
    json!({
        "id": pr.id,
        "title": pr.title,
        "url": pr.url,
        "repo": pr.repo,
        "number": pr.number,
        "isDraft": pr.is_draft,
        "ci": pr.ci.as_str(),
        "review": pr.review.as_str(),
        "author": pr.author,
    })
}

fn repo_json(repo: &RepoCi) -> Value {
    json!({
        "repo": repo.repo,
        "url": repo.url,
        "branch": repo.branch,
        "ci": repo.ci.as_str(),
    })
}

fn activity_json(activity: &Activity) -> Value {
    let weeks: Vec<Value> = activity
        .weeks
        .iter()
        .map(|week| {
            Value::Array(
                week.iter()
                    .map(|day| {
                        json!({
                            "date": day.date,
                            "count": day.count,
                            "level": day.level,
                            "weekday": day.weekday,
                        })
                    })
                    .collect(),
            )
        })
        .collect();
    json!({ "total": activity.total, "weeks": weeks })
}

struct Cache {
    repos: Option<i64>,
    stars: Option<i64>,
    pulse: Option<Pulse>,
    activity: Option<Activity>,
}

static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(Cache {
    repos: None,
    stars: None,
    pulse: None,
    activity: None,
});

pub fn remember_stats(repos: i64, stars: i64) {
    let mut cache = CACHE.lock().unwrap();
    cache.repos = Some(repos);
    cache.stars = Some(stars);
}

/// Stores the pulse and returns alerts against the previous one. The first store is silent.
pub fn remember_pulse(pulse: Pulse) -> Vec<GhEvent> {
    let mut cache = CACHE.lock().unwrap();
    let found = events(cache.pulse.as_ref(), &pulse);
    cache.pulse = Some(pulse);
    found
}

pub fn remember_activity(activity: Activity) {
    CACHE.lock().unwrap().activity = Some(activity);
}

/// One object for the island, so a pulse update does not wipe the star count.
pub fn snapshot() -> Value {
    let cache = CACHE.lock().unwrap();
    let mut data = Map::new();
    if let Some(n) = cache.repos {
        data.insert("totalRepos".into(), json!(n));
    }
    if let Some(n) = cache.stars {
        data.insert("totalStars".into(), json!(n));
    }
    if let Some(pulse) = &cache.pulse {
        data.insert("login".into(), json!(pulse.login));
        data.insert(
            "myPRs".into(),
            Value::Array(pulse.my_prs.iter().map(pr_json).collect()),
        );
        data.insert(
            "toReview".into(),
            Value::Array(pulse.to_review.iter().map(pr_json).collect()),
        );
        data.insert(
            "mainCI".into(),
            Value::Array(pulse.main_ci.iter().map(repo_json).collect()),
        );
    }
    if let Some(activity) = &cache.activity {
        data.insert("activity".into(), activity_json(activity));
    }
    Value::Object(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
      "data": {
        "viewer": {
          "login": "testuser",
          "pullRequests": { "nodes": [
            {
              "number": 42, "title": "Add feature",
              "url": "https://github.com/testuser/myrepo/pull/42",
              "isDraft": false, "reviewDecision": "APPROVED",
              "repository": {"nameWithOwner": "testuser/myrepo"},
              "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
            },
            {
              "number": 43, "title": "Fix bug",
              "url": "https://github.com/testuser/myrepo/pull/43",
              "isDraft": true, "reviewDecision": null,
              "repository": {"nameWithOwner": "testuser/myrepo"},
              "commits": {"nodes": [{"commit": {"statusCheckRollup": null}}]}
            }
          ]},
          "repositories": { "nodes": [
            {
              "nameWithOwner": "testuser/myrepo",
              "url": "https://github.com/testuser/myrepo",
              "isArchived": false,
              "defaultBranchRef": {"name": "main", "target": {"statusCheckRollup": {"state": "PENDING"}}}
            },
            {
              "nameWithOwner": "testuser/archived",
              "url": "https://github.com/testuser/archived",
              "isArchived": true,
              "defaultBranchRef": {"name": "main", "target": {"statusCheckRollup": {"state": "SUCCESS"}}}
            }
          ]}
        },
        "reviewRequested": { "nodes": [
          {
            "number": 7, "title": "Review this",
            "url": "https://github.com/other/repo/pull/7",
            "isDraft": false,
            "author": {"login": "otheruser"},
            "repository": {"nameWithOwner": "other/repo"}
          }
        ]}
      }
    }"#;

    fn json(raw: &str) -> Value {
        serde_json::from_str(raw).unwrap()
    }

    fn blank() -> Pulse {
        Pulse { login: "t".into(), my_prs: vec![], to_review: vec![], main_ci: vec![] }
    }

    fn pr(id: &str, ci: Ci, sha: Option<&str>) -> PullRequest {
        PullRequest {
            id: id.into(),
            title: "T".into(),
            url: String::new(),
            repo: "r/p".into(),
            number: 1,
            is_draft: false,
            ci,
            review: Review::Unknown,
            head_sha: sha.map(str::to_string),
            author: None,
        }
    }

    #[test]
    fn parses_pulse_and_drops_archived_repos() {
        let pulse = parse_pulse(&json(VALID)).unwrap();
        assert_eq!(pulse.login, "testuser");
        assert_eq!(pulse.my_prs.len(), 2);
        assert_eq!(pulse.my_prs[0].id, "testuser/myrepo#42");
        assert_eq!(pulse.my_prs[0].ci, Ci::Success);
        assert_eq!(pulse.my_prs[0].review, Review::Approved);
        assert_eq!(pulse.my_prs[1].ci, Ci::Unknown);
        assert!(pulse.my_prs[1].is_draft);
        assert_eq!(pulse.main_ci.len(), 1);
        assert_eq!(pulse.main_ci[0].repo, "testuser/myrepo");
        assert_eq!(pulse.main_ci[0].ci, Ci::Pending);
        assert_eq!(pulse.main_ci[0].branch, "main");
        assert_eq!(pulse.to_review.len(), 1);
        assert_eq!(pulse.to_review[0].id, "other/repo#7");
        assert!(pulse.has_pending());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_pulse(&json("{}")).is_none());
        assert!(parse_pulse(&Value::String("garbage".into())).is_none());
    }

    #[test]
    fn ci_words() {
        assert_eq!(Ci::parse(None), Ci::Unknown);
        assert_eq!(Ci::parse(Some("PENDING")), Ci::Pending);
        assert_eq!(Ci::parse(Some("EXPECTED")), Ci::Pending);
        assert_eq!(Ci::parse(Some("SUCCESS")), Ci::Success);
        assert_eq!(Ci::parse(Some("FAILURE")), Ci::Failure);
        assert_eq!(Ci::parse(Some("ERROR")), Ci::Failure);
        assert_eq!(Ci::parse(Some("WAITING")), Ci::Unknown);
    }

    #[test]
    fn first_poll_is_silent() {
        let pulse = parse_pulse(&json(VALID)).unwrap();
        assert!(events(None, &pulse).is_empty());
    }

    #[test]
    fn pending_to_success_and_success_to_failure() {
        let mut old = blank();
        old.my_prs = vec![pr("r/p#1", Ci::Pending, None)];
        let mut new = old.clone();
        new.my_prs[0].ci = Ci::Success;
        assert_eq!(
            events(Some(&old), &new),
            vec![GhEvent::CiPassed { pr_id: "r/p#1".into() }]
        );

        old.my_prs = vec![pr("r/p#2", Ci::Success, None)];
        new.my_prs = vec![pr("r/p#2", Ci::Failure, None)];
        assert_eq!(
            events(Some(&old), &new),
            vec![GhEvent::CiFailed { pr_id: "r/p#2".into() }]
        );
    }

    #[test]
    fn new_sha_alerts_immediately_and_main_stays_quiet_when_green() {
        let old = blank();
        let mut new = blank();
        new.my_prs = vec![pr("r/p#10", Ci::Success, Some("abc"))];
        assert_eq!(
            events(Some(&old), &new),
            vec![GhEvent::CiPassed { pr_id: "r/p#10".into() }]
        );

        new.my_prs = vec![pr("r/p#11", Ci::Pending, Some("abc"))];
        assert!(events(Some(&old), &new).is_empty());

        let mut old = blank();
        old.my_prs = vec![pr("r/p#12", Ci::Success, Some("sha-old"))];
        let mut new = old.clone();
        new.my_prs[0].ci = Ci::Failure;
        new.my_prs[0].head_sha = Some("sha-new".into());
        assert_eq!(
            events(Some(&old), &new),
            vec![GhEvent::CiFailed { pr_id: "r/p#12".into() }]
        );

        let mut old = blank();
        old.main_ci = vec![RepoCi {
            repo: "a/b".into(),
            url: String::new(),
            branch: "main".into(),
            ci: Ci::Success,
            head_sha: Some("sha-old".into()),
        }];
        let mut green = old.clone();
        green.main_ci[0].head_sha = Some("sha-new".into());
        assert!(events(Some(&old), &green).is_empty());

        let mut red = old.clone();
        red.main_ci[0].ci = Ci::Failure;
        red.main_ci[0].head_sha = Some("sha-new".into());
        assert_eq!(
            events(Some(&old), &red),
            vec![GhEvent::MainFailed { repo: "a/b".into() }]
        );
    }

    #[test]
    fn duplicate_pr_keeps_the_first() {
        let raw = r#"{
          "data": {
            "viewer": {
              "login": "testuser",
              "pullRequests": { "nodes": [
                {
                  "number": 42, "title": "Add feature",
                  "url": "https://github.com/testuser/myrepo/pull/42",
                  "isDraft": false, "reviewDecision": "APPROVED",
                  "repository": {"nameWithOwner": "testuser/myrepo"},
                  "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
                },
                {
                  "number": 42, "title": "Duplicate entry",
                  "url": "https://github.com/testuser/myrepo/pull/42",
                  "isDraft": true,
                  "repository": {"nameWithOwner": "testuser/myrepo"},
                  "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "FAILURE"}}}]}
                }
              ]},
              "repositories": {"nodes": []}
            },
            "reviewRequested": {"nodes": []}
          }
        }"#;
        let pulse = parse_pulse(&json(raw)).unwrap();
        assert_eq!(pulse.my_prs.len(), 1);
        assert_eq!(pulse.my_prs[0].title, "Add feature");
        assert_eq!(pulse.my_prs[0].ci, Ci::Success);
    }

    const ACTIVITY: &str = r#"{
      "data": { "viewer": { "login": "testuser", "contributionsCollection": { "contributionCalendar": {
        "totalContributions": 42,
        "weeks": [
          { "contributionDays": [
            {"date": "2026-01-05", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 0},
            {"date": "2026-01-06", "contributionCount": 1, "contributionLevel": "FIRST_QUARTILE", "weekday": 1},
            {"date": "2026-01-07", "contributionCount": 4, "contributionLevel": "SECOND_QUARTILE", "weekday": 2},
            {"date": "2026-01-08", "contributionCount": 8, "contributionLevel": "THIRD_QUARTILE", "weekday": 3},
            {"date": "2026-01-09", "contributionCount": 12, "contributionLevel": "FOURTH_QUARTILE", "weekday": 4},
            {"date": "2026-01-10", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 5},
            {"date": "2026-01-11", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 6}
          ]},
          { "contributionDays": [
            {"date": "2026-01-12", "contributionCount": 5, "contributionLevel": "SECOND_QUARTILE", "weekday": 0},
            {"date": "2026-01-13", "contributionCount": 10, "contributionLevel": "THIRD_QUARTILE", "weekday": 1}
          ]}
        ]
      }}} }
    }"#;

    #[test]
    fn parses_activity_levels_and_windows() {
        let act = parse_activity(&json(ACTIVITY)).unwrap();
        assert_eq!(act.total, 42);
        assert_eq!(act.weeks.len(), 2);
        assert_eq!(act.weeks[0].len(), 7);
        assert_eq!(act.weeks[0][0].level, 0);
        assert_eq!(act.weeks[0][4].level, 4);
        assert_eq!(act.last_days(3)[0].date, "2026-01-11");
        assert_eq!(act.last_days(3)[2].date, "2026-01-13");
        assert_eq!(act.last_days(99).len(), 9);
        assert!(act.last_days(0).is_empty());
        assert_eq!(act.last_weeks(1).len(), 1);
        assert_eq!(act.last_weeks(1)[0].len(), 2);
        assert!(act.last_weeks(0).is_empty());

        let unknown = r#"{
          "data": { "viewer": { "contributionsCollection": { "contributionCalendar": {
            "totalContributions": 1,
            "weeks": [{ "contributionDays": [
              {"date": "2026-03-01", "contributionCount": 1, "contributionLevel": "EXTRA_SPECIAL", "weekday": 0}
            ]}]
          }}} }
        }"#;
        assert_eq!(parse_activity(&json(unknown)).unwrap().weeks[0][0].level, 0);
        assert!(parse_activity(&json("{}")).is_none());
    }

    #[test]
    fn staleness() {
        assert!(is_stale(None, 1_000, 60_000));
        assert!(!is_stale(Some(1_000), 1_000, 60_000));
        assert!(is_stale(Some(0), 61_000, 60_000));
    }
}
