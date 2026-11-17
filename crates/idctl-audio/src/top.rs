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
    let (id, quantum, rate, load) = (at("ID")?, at("QUANT")?, at("RATE")?, at("B/Q")?);
    let (errors, format, name) = (at("ERR")?, at("FORMAT")?, at("NAME")?);
    // The real header starts `S ID QUANT RATE` and ends `ERR FORMAT NAME`.
    // Anything else, such as a node name that mentions those words, is not
    // one: its positions could point past the end of a row.
    let in_order = (id, quantum, rate) == (1, 2, 3)
        && rate < load
        && load < errors
        && errors < format
        && format < name;
    in_order.then_some(Header {
        id,
        quantum,
        rate,
        load,
        errors,
        skip: format - errors - 1,
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
    // `C`: pw-top has not heard from the node yet and prints zeroed counters.
    if cols.first() == Some(&"C") {
        return None;
    }
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
        id: cols.get(header.id)?.parse().ok()?,
        quantum: cols.get(header.quantum)?.parse().ok()?,
        rate: cols.get(header.rate)?.parse().ok()?,
        load: cols
            .get(header.load)?
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite()),
        errors: cols.get(header.errors)?.parse().ok()?,
        follower,
        name: rest.join(" "),
    })
}

/// Rows kept from one iteration; a graph has tens of nodes, so more is noise.
const MAX_ROWS: usize = 4096;

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
        if self.rows.len() < MAX_ROWS {
            self.rows.push(row);
        }
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
    fn rows_still_being_created_are_skipped() {
        // pw-top's first iteration shows every node as `C` with zeroed
        // counters; taking those as a baseline would make old xruns look new.
        assert!(row(
            "C  150      0      0    ---     ---   ---   ---     0                  My DAW"
        )
        .is_none());
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
        assert!(done[0].is_empty());
        assert!(done[1..].iter().all(|f| f.len() == 5));
        let daw = |f: &[Row]| f.iter().find(|r| r.name == "My DAW").unwrap().errors;
        assert_eq!((daw(&done[1]), daw(&done[2])), (3, 5));
    }

    #[test]
    fn a_real_idle_capture_parses() {
        let mut frames = Frames::default();
        let done: Vec<Vec<Row>> = IDLE.lines().filter_map(|l| frames.push(l)).collect();
        assert_eq!(done.len(), 2);
        assert!(done[1].iter().any(|r| r.name.contains("iD2")));
    }

    #[test]
    fn a_header_out_of_order_is_refused() {
        assert!(
            parse_header("S ERR FORMAT NAME a b c d e f g h i j k ID QUANT RATE B/Q").is_none()
        );
        assert!(parse_header("S QUANT ID RATE WAIT BUSY W/Q B/Q ERR FORMAT NAME").is_none());
    }

    #[test]
    fn a_node_row_is_never_a_header() {
        let forged = "S   44      0      0    ---     ---   ---   ---     0   ID QUANT RATE B/Q ERR FORMAT NAME";
        assert!(parse_header(forged).is_none());
    }

    #[test]
    fn a_row_shorter_than_its_header_is_skipped() {
        let wide = parse_header("S ID QUANT RATE WAIT BUSY W/Q B/Q ERR FORMAT NAME").unwrap();
        for line in ["R 44", "R 44 64 48000 1us", "R 44 64 48000 1us 2us 0.1 0.2"] {
            assert!(parse_row(&wide, line).is_none(), "{line}");
        }
    }

    #[test]
    fn one_iteration_keeps_a_bounded_number_of_rows() {
        let mut frames = Frames::default();
        frames.push(HEADER);
        for id in 0..10_000 {
            frames.push(&format!(
                "R {id} 64 48000 1.0us 1.0us 0.01 0.01 0 F32LE 2 48000 = node{id}"
            ));
        }
        assert_eq!(frames.push(HEADER).unwrap().len(), 4096);
    }
}
