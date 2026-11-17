//! `pw-top -b` output. One header line starts each iteration; every other
//! line is a node, with followers listed under their driver.

/// One node in one iteration.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: u32,
    /// Frames per cycle; 0 while the node is idle.
    pub quantum: u32,
    pub rate: u32,
    /// Share of the period spent processing (the `B/Q` column).
    pub load: Option<f32>,
    /// Xruns since the node was created.
    pub errors: u64,
    /// Driven by the driver row above it.
    pub follower: bool,
    pub name: String,
}

/// Where each column we read sits in a row.
#[derive(Clone, Debug)]
pub struct Header {
    id: usize,
    quantum: usize,
    rate: usize,
    load: usize,
    errors: usize,
    /// Columns between `ERR` and `FORMAT` that we do not know.
    skip: usize,
}

pub fn parse_header(line: &str) -> Option<Header> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    let at = |name: &str| cols.iter().position(|c| *c == name);
    if cols.first() != Some(&"S") || at("NAME").is_none() {
        return None;
    }
    let errors = at("ERR")?;
    Some(Header {
        id: at("ID")?,
        quantum: at("QUANT")?,
        rate: at("RATE")?,
        load: at("B/Q")?,
        errors,
        skip: at("FORMAT")?.checked_sub(errors + 1)?,
    })
}

/// A sample format as pw-top prints it: `S32LE`, `F32LE`, `S24_32LE`.
fn is_format(token: &str) -> bool {
    token.starts_with(['S', 'U', 'F'])
        && token.chars().any(|c| c.is_ascii_digit())
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

pub fn parse_row(header: &Header, line: &str) -> Option<Row> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    let mut rest = cols.get(header.errors + 1 + header.skip..)?;
    if let [format, channels, rate, ..] = rest {
        if is_format(format) && channels.parse::<u32>().is_ok() && rate.parse::<u32>().is_ok() {
            rest = &rest[3..];
        }
    }
    let follower = matches!(rest.first(), Some(&"=") | Some(&"+"));
    if follower {
        rest = &rest[1..];
    }
    if rest.is_empty() {
        return None;
    }
    Some(Row {
        id: cols[header.id].parse().ok()?,
        quantum: cols[header.quantum].parse().ok()?,
        rate: cols[header.rate].parse().ok()?,
        load: cols[header.load]
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite()),
        errors: cols[header.errors].parse().ok()?,
        follower,
        name: rest.join(" "),
    })
}

/// Groups lines into iterations.
#[derive(Default)]
pub struct Frames {
    header: Option<Header>,
    rows: Vec<Row>,
}

impl Frames {
    /// Feed one line. Returns the finished iteration when the next header
    /// arrives.
    pub fn push(&mut self, line: &str) -> Option<Vec<Row>> {
        if let Some(header) = parse_header(line) {
            let done = self
                .header
                .replace(header)
                .map(|_| std::mem::take(&mut self.rows));
            self.rows.clear();
            return done;
        }
        let row = parse_row(self.header.as_ref()?, line)?;
        self.rows.push(row);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR FORMAT           NAME ";
    const IDLE: &str = include_str!("../fixtures/top-idle.txt");
    const DRIVING: &str = include_str!("../fixtures/top-driving.txt");

    fn row(line: &str) -> Option<Row> {
        parse_row(&parse_header(HEADER).unwrap(), line)
    }

    #[test]
    fn header_columns_are_found_by_name() {
        assert!(parse_header(HEADER).is_some());
        assert!(parse_header("R   44     64  48000").is_none());
        let wide =
            parse_header("S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  XTRA  ERR FORMAT NAME")
                .unwrap();
        let r = parse_row(
            &wide,
            "R  150     64  48000 200.1us   1.4ms  0.15  1.04   9.9    3    F32LE 2 48000  = My DAW",
        )
        .unwrap();
        assert_eq!(r.errors, 3);
        assert_eq!(r.load, Some(1.04));
        assert_eq!(r.name, "My DAW");
    }

    #[test]
    fn idle_row_has_no_load() {
        let r = row(
            "S   29      0      0    ---     ---   ---   ---     0                  Dummy-Driver",
        )
        .unwrap();
        assert_eq!((r.id, r.quantum, r.rate, r.errors), (29, 0, 0, 0));
        assert_eq!(r.load, None);
        assert!(!r.follower);
        assert_eq!(r.name, "Dummy-Driver");
    }

    #[test]
    fn follower_marker_and_name_with_spaces() {
        let r =
            row("R  150     64  48000 200.1us   1.4ms  0.15  1.04    3    F32LE 2 48000  = My DAW")
                .unwrap();
        assert!(r.follower);
        assert_eq!(r.name, "My DAW");
        assert_eq!(r.errors, 3);
        assert_eq!(r.load, Some(1.04));
        assert_eq!((r.quantum, r.rate), (64, 48_000));
        let plus =
            row("R   46     64  48000  10.0us   5.0us  0.01  0.02    0    S32LE 12 48000  + in")
                .unwrap();
        assert!(plus.follower);
    }

    #[test]
    fn row_without_format_keeps_its_name() {
        let r = row("R   44     64  48000  41.2us  12.3us  0.03  0.18    0                  alsa_output.usb-X.pro-output-0")
            .unwrap();
        assert_eq!(r.name, "alsa_output.usb-X.pro-output-0");
        assert!(!r.follower);
        assert_eq!(r.load, Some(0.18));
    }

    #[test]
    fn bad_rows_are_skipped() {
        assert!(row("").is_none());
        assert!(row("R   44     64  480").is_none());
        assert!(row("R   xx     64  48000  41.2us  12.3us  0.03  0.18    0    name").is_none());
        assert!(row("R   44     64  48000  41.2us  12.3us  0.03  0.18    0").is_none());
        let comma = row("R   44     64  48000  41,2us  12,3us  0,03  0,31    0    name").unwrap();
        assert_eq!(comma.load, None);
        let odd = row("R   44     64  48000  41.2us  12.3us  0.03  NaN    0    name").unwrap();
        assert_eq!(odd.load, None);
    }

    #[test]
    fn frames_complete_on_the_next_header() {
        let mut frames = Frames::default();
        let done: Vec<Vec<Row>> = DRIVING.lines().filter_map(|l| frames.push(l)).collect();
        assert_eq!(done.len(), 3);
        assert!(done.iter().all(|f| f.len() == 5));
        let daw = |f: &[Row]| f.iter().find(|r| r.name == "My DAW").unwrap().errors;
        assert_eq!((daw(&done[0]), daw(&done[1]), daw(&done[2])), (0, 3, 5));
    }

    #[test]
    fn a_real_idle_capture_parses() {
        let mut frames = Frames::default();
        let done: Vec<Vec<Row>> = IDLE.lines().filter_map(|l| frames.push(l)).collect();
        assert_eq!(done.len(), 1);
        assert!(done[0].iter().any(|r| r.name.contains("iD2")));
    }
}
