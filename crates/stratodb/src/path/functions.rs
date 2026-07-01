use super::segment::{Segment, Segments};
use crate::error::{SdbError, SdbResult};

pub(super) fn validate_name(name: &str, full: &str) -> SdbResult<()> {
    if name.contains(['/', '[', ']']) {
        return Err(SdbError::InvalidPath(format!(
            "reserved character in segment '{name}' of '{full}'"
        )));
    }

    if name == "." || name == ".." {
        return Err(SdbError::InvalidPath(format!("reserved segment '{name}' in '{full}'")));
    }

    Ok(())
}

pub(super) fn parse_token(token: &str, full: &str, out: &mut Segments) -> SdbResult<()> {
    let Some(bracket) = token.find('[') else {
        validate_name(token, full)?;
        out.push(Segment::Name(token.into()));
        return Ok(());
    };

    let name = &token[..bracket];
    if !name.is_empty() {
        validate_name(name, full)?;
        out.push(Segment::Name(name.into()));
    }

    let mut rest = &token[bracket..];
    while !rest.is_empty() {
        if !rest.starts_with('[') {
            return Err(SdbError::InvalidPath(format!("expected '[' in segment of '{full}'")));
        }

        let close = rest
            .find(']')
            .ok_or_else(|| SdbError::InvalidPath(format!("unclosed '[' in '{full}'")))?;
        let digits = &rest[1..close];
        let index: u64 = digits
            .parse()
            .map_err(|_| SdbError::InvalidPath(format!("invalid index '{digits}' in '{full}'")))?;
        out.push(Segment::Index(index));

        rest = &rest[close + 1..];
    }

    Ok(())
}
