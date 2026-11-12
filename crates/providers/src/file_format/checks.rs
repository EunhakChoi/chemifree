//! Version-gated checks on the raw JSON for the fields whose shared-shape defaults exist only for
//! migration.

use chemifree_domain::document::DocumentParts;
use chemifree_domain::scene::SceneObject;

use super::{FORMAT_VERSION, Version, earlier, parse};

/// The shared scene DTO default is migration-only, including every history side.
pub(super) fn check_graphic_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    fn visit(
        value: &serde_json::Value,
        width_field: bool,
        ellipse: bool,
        polygon: bool,
        spline: bool,
        images: bool,
    ) -> Result<(), String> {
        match value {
            serde_json::Value::Object(object) => {
                if !images
                    && object.get("kind").and_then(serde_json::Value::as_str) == Some("image")
                {
                    return Err("Embedded images require native v30 and are forbidden in v1–29, including history.".into());
                }
                if !spline
                    && object.get("kind").and_then(serde_json::Value::as_str) == Some("graphic")
                    && object.get("shape").and_then(serde_json::Value::as_str) == Some("spline")
                {
                    return Err("Spline graphics require native v29 and are forbidden in v1–28, including history.".into());
                }
                if !polygon
                    && object.get("kind").and_then(serde_json::Value::as_str) == Some("graphic")
                    && object.get("shape").and_then(serde_json::Value::as_str) == Some("polygon")
                {
                    return Err("Polygon graphics require native v28 and are forbidden in v1–27, including history.".into());
                }
                if object.get("kind").and_then(serde_json::Value::as_str) == Some("graphic") {
                    if object.contains_key("line_width_pt") != width_field {
                        return Err("Graphic line_width_pt is required in v26+ and forbidden in v1–25, including history.".into());
                    }
                    if !ellipse
                        && object.get("shape").and_then(serde_json::Value::as_str)
                            == Some("ellipse")
                    {
                        return Err("Ellipse graphics require native v27 and are forbidden in v1–26, including history.".into());
                    }
                }
                for child in object.values() {
                    visit(child, width_field, ellipse, polygon, spline, images)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    visit(child, width_field, ellipse, polygon, spline, images)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(
        &parse(bytes, "document or draft")?,
        matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
        ),
        matches!(
            format,
            Version::Current | Version::TwentyNine | Version::TwentyEight | Version::TwentySeven
        ),
        matches!(
            format,
            Version::Current | Version::TwentyNine | Version::TwentyEight
        ),
        matches!(format, Version::Current | Version::TwentyNine),
        format == Version::Current,
    )
}

/// The shared DTO defaults only support migration. v9 requires the fields on every
/// current/history state; earlier headers cannot smuggle even empty v9 fields.
pub(super) fn check_abbreviation_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    let raw: serde_json::Value = parse(bytes, "document or draft")?;
    let current = matches!(
        format,
        Version::Current
            | Version::TwentyNine
            | Version::TwentyEight
            | Version::TwentySeven
            | Version::TwentySix
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
            | Version::Seventeen
            | Version::Sixteen
            | Version::Fifteen
            | Version::Fourteen
            | Version::Thirteen
            | Version::Twelve
            | Version::Eleven
            | Version::Ten
            | Version::Nine
    );
    let document = raw.get("document").ok_or("missing document")?;
    let history = raw.get("history").ok_or("missing history")?;
    let mut states = vec![document];
    for stack in ["undo", "redo"] {
        let entries = history
            .get(stack)
            .and_then(serde_json::Value::as_array)
            .ok_or("missing history stack")?;
        states.extend(entries);
    }
    if states.iter().any(|state| {
        state.get("fragment_displays").is_some()
            != matches!(
                format,
                Version::Current
                    | Version::TwentyNine
                    | Version::TwentyEight
                    | Version::TwentySeven
                    | Version::TwentySix
                    | Version::TwentyFive
                    | Version::TwentyFour
            )
    }) {
        return Err(
            "fragment_displays are required in v24+ and forbidden in v1–23, including history."
                .into(),
        );
    }
    let check_display = |display: &serde_json::Value| -> Result<(), String> {
        if display.is_null() {
            return Ok(());
        }
        let rich = matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
                | Version::TwentyFive
        );
        if display.get("runs").is_some() != rich
            || display.get("text").is_some() == rich
            || (rich && !display["runs"].is_array())
            || (!rich && !display["text"].is_string())
        {
            return Err("Fragment display requires runs without text in v25; v24 requires text without runs, including history.".into());
        }
        Ok(())
    };
    for (index, state) in states.iter().enumerate() {
        if let Some(displays) = state
            .get("fragment_displays")
            .and_then(serde_json::Value::as_array)
        {
            for display in displays {
                if index == 0 {
                    check_display(display)?;
                } else {
                    if let Some(before) = display.get("before") {
                        check_display(before)?;
                    }
                    if let Some(after) = display.get("after") {
                        check_display(after)?;
                    }
                }
            }
        }
    }
    if states.iter().any(|state| {
        state.get("display_labels").is_some()
            != matches!(
                format,
                Version::Current
                    | Version::TwentyNine
                    | Version::TwentyEight
                    | Version::TwentySeven
                    | Version::TwentySix
                    | Version::TwentyFive
                    | Version::TwentyFour
                    | Version::TwentyThree
            )
    }) {
        return Err(
            "display_labels are required in v23+ and forbidden in v1–22, including history.".into(),
        );
    }
    if states
        .iter()
        .any(|state| state.get("abbreviations").is_some() != current)
    {
        return Err(
            "Abbreviation fields are required in v9 and forbidden in v1–8, including history."
                .into(),
        );
    }
    Ok(())
}

/// Defaults are migration-only: v22 requires shadow, v21+ outline, v20+ underline,
/// v19+ style and v18+ weight
/// in every stored settings state.
pub(super) fn check_settings_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    let raw: serde_json::Value = parse(bytes, "document or draft")?;
    let current = matches!(
        format,
        Version::Current
            | Version::TwentyNine
            | Version::TwentyEight
            | Version::TwentySeven
            | Version::TwentySix
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
    );
    let check = |settings: &serde_json::Value| -> Result<(), String> {
        if settings.get("label_weight").is_some() != current
            || (current && !matches!(settings["label_weight"].as_str(), Some("normal" | "bold")))
        {
            return Err("label_weight is required as normal or bold in v18–30 and forbidden in v1–17, including history.".into());
        }
        let styled = matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
                | Version::Twenty
                | Version::Nineteen
        );
        if settings.get("label_style").is_some() != styled
            || (styled && !matches!(settings["label_style"].as_str(), Some("normal" | "italic")))
        {
            return Err("label_style is required as normal or italic in v19–30 and forbidden in v1–18, including history.".into());
        }
        let underlined = matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
                | Version::Twenty
        );
        if settings.get("label_underline").is_some() != underlined
            || (underlined && !settings["label_underline"].is_boolean())
        {
            return Err("label_underline is required as a boolean in v20–30 and forbidden in v1–19, including history.".into());
        }
        let outlined = matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
        );
        if settings.get("label_outline").is_some() != outlined
            || (outlined && !settings["label_outline"].is_boolean())
        {
            return Err("label_outline is required as a boolean in v21–30 and forbidden in v1–20, including history.".into());
        }
        let shadowed = matches!(
            format,
            Version::Current
                | Version::TwentyNine
                | Version::TwentyEight
                | Version::TwentySeven
                | Version::TwentySix
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
        );
        if settings.get("label_shadow").is_some() != shadowed
            || (shadowed && !settings["label_shadow"].is_boolean())
        {
            return Err("label_shadow is required as a boolean in v22–30 and forbidden in v1–21, including history.".into());
        }
        Ok(())
    };
    if let Some(settings) = raw["document"].get("settings") {
        check(settings)?;
    }
    for stack in ["undo", "redo"] {
        if let Some(entries) = raw["history"][stack].as_array() {
            for entry in entries {
                if let Some(settings) = entry.get("settings").filter(|value| !value.is_null()) {
                    check(&settings["before"])?;
                    check(&settings["after"])?;
                }
            }
        }
    }
    Ok(())
}

/// Required caption fields are versioned for every current and historical state.
pub(super) fn check_caption_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    fn check(value: &serde_json::Value, version: u32) -> Result<(), String> {
        let styles = version >= 10;
        let alignment = version >= 11;
        let font = version >= 12;
        let size = version >= 13;
        let color = version >= 14;
        let underline = version >= 15;
        let outline = version >= 16;
        let shadow = version >= 17;
        match value {
            serde_json::Value::Object(object) => {
                if object.get("kind").and_then(serde_json::Value::as_str) == Some("caption") {
                    let field = object.get("alignment");
                    if field.is_some() != alignment || field.is_some_and(serde_json::Value::is_null)
                    {
                        return Err("Caption alignment is required in v11–19 and forbidden in v1–10, including history.".into());
                    }
                    if let Some(runs) = object.get("runs").and_then(serde_json::Value::as_array) {
                        for run in runs {
                            if run.get("shadow").is_some() != shadow
                                || (shadow && !run["shadow"].is_boolean())
                            {
                                return Err("Caption shadow is required as a boolean in v17–19 and forbidden in v1–16, including history.".into());
                            }
                            if run.get("outline").is_some() != outline
                                || (outline && !run["outline"].is_boolean())
                            {
                                return Err("Caption outline is required as a boolean in v16–19 and forbidden in v1–15, including history.".into());
                            }
                            if run.get("underline").is_some() != underline
                                || (underline && !run["underline"].is_boolean())
                            {
                                return Err("Caption underline is required as a boolean in v15–19 and forbidden in v1–14, including history.".into());
                            }
                            if run.get("color").is_some() != color {
                                return Err("Caption color is required in v14–19 and forbidden in v1–13, including history.".into());
                            }
                            if run.get("size_pt").is_some() != size {
                                return Err("Caption size_pt is required in v13–19 and forbidden in v1–12, including history.".into());
                            }
                            if run.get("font").is_some() != font {
                                return Err("Caption font is required in v12–19 and forbidden in v1–11, including history.".into());
                            }
                            for field in ["weight", "style"] {
                                let present = run.get(field).is_some();
                                if present != styles || (present && run[field].is_null()) {
                                    return Err("Caption weight/style are required in v10–19 and forbidden in v1–9, including history.".into());
                                }
                            }
                        }
                    }
                }
                for child in object.values() {
                    check(child, version)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    check(child, version)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    check(
        &parse(bytes, "document or draft")?,
        earlier(Ok(format)).unwrap_or(FORMAT_VERSION),
    )
}

/// Caption variants are forbidden in all earlier versions, including history-only states.
pub(super) fn refuse_earlier_captions(
    format: Version,
    parts: &DocumentParts,
) -> Result<(), String> {
    if matches!(
        format,
        Version::Current
            | Version::TwentyNine
            | Version::TwentyEight
            | Version::TwentySeven
            | Version::TwentySix
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
            | Version::Seventeen
            | Version::Sixteen
            | Version::Fifteen
            | Version::Fourteen
            | Version::Thirteen
            | Version::Twelve
            | Version::Eleven
            | Version::Ten
            | Version::Nine
            | Version::Eight
    ) {
        return Ok(());
    }
    let caption = |object: &SceneObject| matches!(object, SceneObject::Caption(_));
    if parts.objects.iter().any(|(_, object)| caption(object))
        || parts.undo.iter().chain(&parts.redo).any(|change| {
            change.objects.iter().any(|(_, state)| {
                state.before.as_ref().is_some_and(caption)
                    || state.after.as_ref().is_some_and(caption)
            })
        })
    {
        return Err("Captions require file format version 8.".into());
    }
    Ok(())
}
