//! Right-align cursor-surgery split for the bash PS1 channel (niubash#169).
//!
//! GNU bash themes right-align a prompt segment by emitting raw cursor
//! movement inside PS1: a jump to the right margin (`CSI n C` with a count
//! that clamps, or `CSI col G` into the right half), optionally followed by
//! cursor-back moves (`CSI n D`), then the segment text — all *unmarked* by
//! `\[ \]` (oh-my-bash `powerline-multiline.base.sh`: `\033[500C` /
//! `\033[${RIGHT_PROMPT_LENGTH}D`; bash-it `powerline-multiline.base.bash`:
//! the same idiom; wt70 on the rubash side documented the family).
//!
//! GNU readline executes those bytes and its cursor anchoring survives the
//! surgery because redisplay positions relative to the real print head
//! (readline display.c:437-463 strips only the `\[..\]` spans for width).
//! reedline's layout model instead walks the prompt as a LINEAR text run and
//! strips ANSI (`painting/utils.rs:94 line_width` =
//! `strip_ansi(line).width()`, `prompt_lines.rs:95 cursor_pos` /
//! `prompt_lines_with_wrap`): the surgery bytes contribute zero width but
//! move the real print head, so the editor's last-line visible width, row
//! count and margin math detach from what the terminal shows — the cursor
//! accounting disconnect of niubash#169.
//!
//! The fix at the channel is one class rule, not per-theme bytes: when the
//! rendered prompt contains a right-margin jump whose tail (after optional
//! cursor-back moves) is plain styled text — SGR colouring and printable
//! characters only, through the end of that line — the tail is the theme's
//! right prompt. It is returned separately so the line editor places it with
//! its own escape-aware width math, and the consumed jump/back moves are
//! dropped from the left prompt. Shapes whose surgery mixes erases or row
//! moves into the tail (powerbash10k's `CSI G` + `CSI 1K` + `CSI 1A`,
//! save/restore `CSI s`/`CSI u`) do not match the rule and pass through
//! untouched byte-for-byte; their cursor placement keeps relying on the
//! editor's save/restore of the real terminal position.

use unicode_width::UnicodeWidthChar;

/// A minimal scan result: byte range of the right-align surgery plus the
/// line index it happened on.
struct Surgery {
    /// Byte offset where the jump sequence starts (left prompt is cut here).
    jump_start: usize,
    /// Byte offset where the right-prompt tail begins.
    tail_start: usize,
    /// Byte offset where the tail's line ends (its `\n` or the string end);
    /// everything from here on stays in the left prompt.
    line_end: usize,
    /// 0-based index of the line the jump sits on.
    line: usize,
}

/// Scan `rendered` for the right-align surgery idiom. Returns the byte cut
/// points of the LAST (rightmost) match, or `None` when the prompt carries
/// no recognizable surgery.
fn find_surgery(rendered: &str, columns: usize) -> Option<Surgery> {
    let columns = columns.max(1);
    let bytes = rendered.as_bytes();
    let mut col: usize = 0;
    let mut line: usize = 0;
    let mut i = 0usize;

    // State of the current CSI run: `Some((jump_start, jump_end))` once a
    // right-margin jump was seen, reset by any printable text or newline.
    let mut jump: Option<(usize, usize)> = None;
    // Tail candidate start: after the jump, advanced past each cursor-back
    // move; validated and committed only when printable text follows.
    let mut candidate: Option<(usize, usize)> = None;
    let mut best: Option<Surgery> = None;

    while i < bytes.len() {
        match bytes[i] {
            0x1b => {
                let (seq_end, op) = scan_escape(rendered, i);
                if let Some(op) = op {
                    match op {
                        EscOp::Csi { params, final_byte } => {
                            match final_byte {
                                // CUF — cursor forward: a jump when it
                                // reaches (or clamps at) the right margin.
                                b'C' => {
                                    let n = leading_count(params).unwrap_or(1);
                                    if col + n >= columns {
                                        jump = Some((i, seq_end));
                                        candidate = Some((i, seq_end));
                                    } else {
                                        reset_candidate(&mut jump, &mut candidate);
                                    }
                                }
                                // CHA — column horizontal absolute (1-based):
                                // a jump when it moves rightward into the
                                // right half of the line (the bash
                                // right-align idiom places the tail there).
                                b'G' => {
                                    let target = leading_count(params).unwrap_or(1);
                                    let target0 = target.saturating_sub(1);
                                    if target0 >= columns / 2 && target0 > col {
                                        jump = Some((i, seq_end));
                                        candidate = Some((i, seq_end));
                                    } else {
                                        reset_candidate(&mut jump, &mut candidate);
                                    }
                                }
                                // CUB — cursor back: part of the surgery,
                                // advances the tail candidate.
                                b'D' if jump.is_some() => {
                                    if let Some((_, ref mut tail)) = candidate {
                                        *tail = seq_end;
                                    }
                                }
                                // Any other control sequence between the jump
                                // and the text (erase, row moves, save/
                                // restore, SGR is handled below) disqualifies
                                // the idiom: the tail is no longer plain
                                // styled text.
                                _ if candidate.is_some() && final_byte != b'm' => {
                                    reset_candidate(&mut jump, &mut candidate);
                                }
                                _ => {}
                            }
                        }
                        // Non-CSI escapes (DECSC/DECRC, charset, OSC…) break
                        // the idiom the same way.
                        EscOp::Other if candidate.is_some() => {
                            reset_candidate(&mut jump, &mut candidate);
                        }
                        _ => {}
                    }
                }
                i = seq_end;
            }
            b'\n' => {
                reset_candidate(&mut jump, &mut candidate);
                col = 0;
                line += 1;
                i += 1;
            }
            b'\r' => {
                col = 0;
                reset_candidate(&mut jump, &mut candidate);
                i += 1;
            }
            // Prompt-ignore markers and other C0 controls: zero width.
            b if b < 0x20 || b == 0x7f => {
                i += 1;
            }
            _ => {
                // Printable run: a printable character right after the
                // surgery makes the tail candidate decidable — validate the
                // whole tail region (to this line's end) and commit when it
                // is plain styled text. (Markers may sit between the surgery
                // and the text; they are zero-width and never reset the
                // candidate.)
                if let Some((jump_start, tail_start)) = candidate {
                    let line_end = rendered[tail_start..]
                        .find('\n')
                        .map_or(rendered.len(), |rel| tail_start + rel);
                    if tail_is_clean_styled_text(&rendered[tail_start..line_end]) {
                        best = Some(Surgery {
                            jump_start,
                            tail_start,
                            line_end,
                            line,
                        });
                    }
                    reset_candidate(&mut jump, &mut candidate);
                }
                let ch = rendered[i..].chars().next().unwrap_or('\u{fffd}');
                col = col.saturating_add(ch.width().unwrap_or(0));
                i += ch.len_utf8();
            }
        }
    }
    best
}

fn reset_candidate(jump: &mut Option<(usize, usize)>, candidate: &mut Option<(usize, usize)>) {
    *jump = None;
    *candidate = None;
}

enum EscOp<'a> {
    Csi { params: &'a str, final_byte: u8 },
    Other,
}

/// Scan the escape sequence starting at `bytes[start] == 0x1b`; return the
/// byte offset just past it and, for CSI, its parameter text and final byte.
fn scan_escape(rendered: &str, start: usize) -> (usize, Option<EscOp<'_>>) {
    let bytes = rendered.as_bytes();
    let mut i = start + 1;
    if i >= bytes.len() {
        return (i, None);
    }
    match bytes[i] {
        b'[' => {
            i += 1;
            let params_start = i;
            while i < bytes.len() && (0x30..=0x3f).contains(&bytes[i]) {
                i += 1;
            }
            let params = &rendered[params_start..i];
            while i < bytes.len() && (0x20..=0x2f).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() && (0x40..=0x7e).contains(&bytes[i]) {
                let final_byte = bytes[i];
                (i + 1, Some(EscOp::Csi { params, final_byte }))
            } else {
                (i, None)
            }
        }
        b']' => {
            // OSC: terminated by BEL or ST.
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    0x07 => {
                        i += 1;
                        break;
                    }
                    0x1b if i + 1 < bytes.len() && bytes[i + 1] == b'\\' => {
                        i += 2;
                        break;
                    }
                    _ => i += 1,
                }
            }
            (i, Some(EscOp::Other))
        }
        // Two-byte escapes (ESC 7, ESC 8, ESC c, …); three-byte charset
        // designators (ESC ( B) are covered by the intermediate range.
        _ => {
            i += 1;
            while i < bytes.len() && (0x20..=0x2f).contains(&bytes[i]) {
                i += 1;
            }
            (i, Some(EscOp::Other))
        }
    }
}

/// Leading numeric parameter of a CSI sequence (`500` from `500`, `1` from
/// an empty parameter list).
fn leading_count(params: &str) -> Option<usize> {
    let digits: String = params
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Whether `tail` (the candidate region through the end of its line) is the
/// idiom's plain styled text: SGR colouring (`CSI … m`), prompt-ignore
/// markers, and printable characters — with at least one printable. Any
/// other control sequence (erase, row moves, save/restore, OSC) disqualifies
/// the split: the bytes pass through untouched.
fn tail_is_clean_styled_text(tail: &str) -> bool {
    let mut has_printable = false;
    let bytes = tail.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            0x1b => {
                let (seq_end, op) = scan_escape(tail, i);
                match op {
                    Some(EscOp::Csi {
                        final_byte: b'm', ..
                    }) => {}
                    _ => return false,
                }
                i = seq_end;
            }
            0x01 | 0x02 => i += 1,
            b if b < 0x20 || b == 0x7f => return false,
            _ => {
                has_printable = true;
                let ch = tail[i..].chars().next().unwrap_or('\u{fffd}');
                i += ch.len_utf8();
            }
        }
    }
    has_printable
}

/// Split a rendered bash prompt at the right-align surgery. Returns
/// `(left, Some(right))` on a match — `left` keeps every byte before the
/// jump plus the rest of the tail's line (its `\n` and any following
/// lines), `right` carries the aligned tail (SGR colouring included); the
/// jump/back bytes themselves are dropped. Without a match the prompt is
/// returned unchanged. `right_on_last_line` reports whether the tail sits
/// on the prompt's final line (the editor renders its right prompt there).
pub(crate) fn split_right_align(rendered: &str, columns: u16) -> SplitPrompt {
    let Some(surgery) = find_surgery(rendered, columns as usize) else {
        return SplitPrompt {
            left: rendered.to_string(),
            right: None,
            right_on_last_line: false,
        };
    };
    let total_lines = rendered.lines().count();
    // The editor renders its right prompt on the prompt's FIRST or LAST line
    // only; a surgery tail on a middle line would move to the wrong row, so
    // such prompts pass through untouched.
    let on_last_line = total_lines.saturating_sub(1) == surgery.line;
    if surgery.line != 0 && !on_last_line {
        return SplitPrompt {
            left: rendered.to_string(),
            right: None,
            right_on_last_line: false,
        };
    }
    SplitPrompt {
        left: rendered[..surgery.jump_start].to_string() + &rendered[surgery.line_end..],
        right: Some(rendered[surgery.tail_start..surgery.line_end].to_string()),
        right_on_last_line: on_last_line,
    }
}

/// The split outcome for one rendered prompt string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitPrompt {
    pub left: String,
    pub right: Option<String>,
    pub right_on_last_line: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: u16 = 80;

    #[test]
    fn plain_prompt_passes_through() {
        let rendered = "\u{1}\u{1b}[36m\u{2}repo/path\u{1}\u{1b}[0m\u{2}\nP3$ ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, rendered);
        assert_eq!(split.right, None);
        assert!(!split.right_on_last_line);
    }

    #[test]
    fn oh_my_bash_cuf_jump_splits_the_tail() {
        // oh-my-bash powerline-multiline: \033[500C + \033[${N}D + tail.
        let rendered = concat!(
            "\u{1}\u{1b}[34;1m\u{2}\u{f07c} \u{1}\u{1b}[0m\u{2}",
            " D:/repo/start \u{1}\u{1b}[0;30m\u{2}",
            "\u{1b}[500C\u{1b}[11D",
            "\u{1}[~\u{1b}[33;1m 12:34 \u{1b}[0m",
            "\n\u{1}\u{1b}[32;1m\u{2}\u{276f} ",
        );
        let split = split_right_align(rendered, COLS);
        assert!(
            !split.left.contains("\u{1b}[500C"),
            "jump must be dropped from the left: {:?}",
            split.left
        );
        assert!(
            !split.left.contains("\u{1b}[11D"),
            "back-move must be dropped from the left: {:?}",
            split.left
        );
        assert_eq!(split.right.unwrap(), "\u{1}[~\u{1b}[33;1m 12:34 \u{1b}[0m");
        // The jump is on the top line, not the input line.
        assert!(!split.right_on_last_line);
        // The input line stays intact in the left prompt.
        assert!(split.left.contains('\n'));
        assert!(split.left.ends_with("\u{276f} "));
    }

    #[test]
    fn bash_it_cha_style_cuf_with_padding_splits() {
        // bash-it powerline-multiline: RIGHT_PAD + \e[500C + \e[ND + tail.
        let rendered = "path  \u{1b}[500C\u{1b}[9D right-seg \u{1b}[0m\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, "path  \n\u{276f} ");
        assert_eq!(split.right.as_deref(), Some(" right-seg \u{1b}[0m"));
    }

    #[test]
    fn cha_into_right_half_is_a_jump() {
        // The \e[${cols}G idiom (powerbash10k uses it before an erase, which
        // disqualifies; this case has a clean tail instead).
        let rendered = "left \u{1b}[72G\u{1b}[8D clock \u{1b}[0m\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, "left \n\u{276f} ");
        assert_eq!(split.right.as_deref(), Some(" clock \u{1b}[0m"));
    }

    #[test]
    fn powerbash10k_erase_up_tail_is_not_plain_text_and_passes_through() {
        // powerbash10k: 80-char filler wraps, then CSI 80 G, CSI 1 K, CSI 1 A,
        // CSI n D, tail. The erase/row-move between jump and tail disqualifies
        // the idiom: bytes must pass through untouched.
        let filler = "\u{b7}".repeat(80);
        let rendered =
            format!("segs {filler}\u{1b}[80G\u{1b}[1K\u{1b}[1A\u{1b}[17D tail\n\u{276f} ");
        let split = split_right_align(&rendered, COLS);
        assert_eq!(split.left, rendered);
        assert_eq!(split.right, None);
    }

    #[test]
    fn save_restore_idiom_passes_through() {
        // The \e[s .. \e[u bracket would leave a dangling restore in the
        // editor-managed tail; not the plain-text idiom, keep the bytes.
        let rendered = "left \u{1b}[s\u{1b}[75C\u{1b}[5D right \u{1b}[u\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, rendered);
        assert_eq!(split.right, None);
    }

    #[test]
    fn forward_move_with_room_is_not_a_jump() {
        // \e[10C with 60 free columns is ordinary inline movement.
        let rendered = "left \u{1b}[10C mid\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, rendered);
        assert_eq!(split.right, None);
    }

    #[test]
    fn cjk_tail_width_feeds_the_margin_test() {
        // A wide-CJK tail: the jump still clamps at the margin regardless of
        // the tail's width; the split must keep the tail byte-exact.
        let rendered = "left \u{1b}[500C\u{1b}[10D \u{9879}\u{76ee} \u{1b}[0m\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.right.as_deref(), Some(" \u{9879}\u{76ee} \u{1b}[0m"));
        assert!(!split.left.contains("\u{1b}[500C"));
    }

    #[test]
    fn jump_on_final_line_reports_right_on_last_line() {
        // A single-line PS1 whose right-aligned tail shares the input line.
        let rendered = "path \u{1b}[500C\u{1b}[6D 17:04\n";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.right.as_deref(), Some(" 17:04"));
        // The line's newline stays with the left prompt.
        assert_eq!(split.left, "path \n");
        assert!(split.right_on_last_line);
    }

    #[test]
    fn sgr_inside_tail_is_kept() {
        let rendered = "path \u{1b}[500C\u{1b}[8D\u{1b}[33;1m 12:34 \u{1b}[0m\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.right.as_deref(), Some("\u{1b}[33;1m 12:34 \u{1b}[0m"));
    }

    #[test]
    fn narrow_terminal_still_detects_the_clamping_jump() {
        let rendered = "left \u{1b}[500C\u{1b}[4D tail\n\u{276f} ";
        let split = split_right_align(rendered, 20);
        assert_eq!(split.right.as_deref(), Some(" tail"));
    }

    #[test]
    fn middle_line_jump_passes_through() {
        // The editor can only place its right prompt on the first or last
        // line; a tail on a middle line would move to the wrong row.
        let rendered = "top\nmid \u{1b}[500C\u{1b}[4D tail\n\u{276f} ";
        let split = split_right_align(rendered, COLS);
        assert_eq!(split.left, rendered);
        assert_eq!(split.right, None);
    }
}
