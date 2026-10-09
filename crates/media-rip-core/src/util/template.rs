//! Filename templates with explicit conditional groups. Values are inserted once,
//! so braces in metadata are always literal text.

pub const DEFAULT_MOVIE_TEMPLATE: &str = "{movie name}{if:movie year} ({movie year}){end}";

/// Render fields and nested `{if:field}...{end}` groups. A condition is true
/// when the named field is non-empty after trimming. Unknown fields and malformed
/// groups are rejected, including inside a group whose condition is false.
pub fn render_template(template: &str, fields: &[(&str, &str)]) -> Result<String, String> {
    let mut output = String::new();
    let mut active = vec![true];
    let mut remaining = template;
    while let Some((literal, after)) = remaining.split_once('{') {
        if literal.contains('}') {
            return Err("Filename template has an unmatched closing brace.".into());
        }
        if *active.last().unwrap() {
            output.push_str(literal);
        }
        let (token, rest) = after
            .split_once('}')
            .ok_or("Filename template has an unmatched opening brace.")?;
        if token == "end" {
            if active.len() == 1 {
                return Err("Filename template has {end} without {if:field}.".into());
            }
            active.pop();
        } else if let Some(field) = token.strip_prefix("if:") {
            let value = field_value(field, fields)?;
            active.push(*active.last().unwrap() && !value.trim().is_empty());
        } else {
            let value = field_value(token, fields)?;
            if *active.last().unwrap() {
                output.push_str(value);
            }
        }
        remaining = rest;
    }
    if remaining.contains('}') {
        return Err("Filename template has an unmatched closing brace.".into());
    }
    if active.len() != 1 {
        return Err("Filename template is missing {end} for an {if:field} group.".into());
    }
    if *active.last().unwrap() {
        output.push_str(remaining);
    }
    Ok(output)
}

fn field_value<'a>(field: &str, fields: &[(&str, &'a str)]) -> Result<&'a str, String> {
    fields
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("Unknown filename field: {{{field}}}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_groups_own_their_punctuation() {
        for (template, present, absent) in [
            (DEFAULT_MOVIE_TEMPLATE, "Arrival (2016)", "Arrival"),
            (
                "{movie name}{if:movie year} - [{movie year}]{end}",
                "Arrival - [2016]",
                "Arrival",
            ),
            (
                "{if:movie year}{movie year}/{end}{movie name}",
                "2016/Arrival",
                "Arrival",
            ),
        ] {
            assert_eq!(
                render_template(
                    template,
                    &[("movie name", "Arrival"), ("movie year", "2016")]
                )
                .unwrap(),
                present
            );
            assert_eq!(
                render_template(template, &[("movie name", "Arrival"), ("movie year", "")])
                    .unwrap(),
                absent
            );
        }
    }

    #[test]
    fn nested_conditions_and_metadata_are_literal() {
        let template = "{if:movie name}{movie name}{if:movie year} ({movie year}){end}{end}";
        assert_eq!(
            render_template(
                template,
                &[("movie name", "{if:movie year}"), ("movie year", "")]
            )
            .unwrap(),
            "{if:movie year}"
        );
        assert_eq!(
            render_template(template, &[("movie name", " "), ("movie year", "2016")]).unwrap(),
            ""
        );
        assert_eq!(
            render_template(
                "{movie name} ({movie year})",
                &[("movie name", "Arrival"), ("movie year", "")]
            )
            .unwrap(),
            "Arrival ()"
        );
    }

    #[test]
    fn rejects_malformed_and_unknown_tokens_even_in_hidden_groups() {
        for template in [
            "{end}",
            "{if:movie year}x",
            "{movie name",
            "x}",
            "{unknown}",
            "{if:unknown}x{end}",
            "{if:movie year}{unknown}{end}",
        ] {
            assert!(
                render_template(template, &[("movie name", "Arrival"), ("movie year", "")])
                    .is_err(),
                "{template}"
            );
        }
    }
}
