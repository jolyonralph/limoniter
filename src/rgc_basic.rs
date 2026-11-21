//! RGC BASIC, Retro Game Coders' modern BASIC (github.com/omiq/rgc-basic): Commodore BASIC's ways with structured
//! blocks and words of its own for sprites, sound and music, and the screen. F9 runs a program with its desktop
//! interpreter, `basic-gfx program.bas` (its window of 40 by 25 PETSCII characters, with graphics and sound), as
//! its README gives it; Get an emulator fetches the interpreter from its GitHub releases. File, New has a starter.

/// The words offered as RGC BASIC is typed: its statements and its functions.
pub fn keywords() -> Vec<String> {
    let mut words: Vec<String> = WORDS.split_whitespace().map(str::to_string).collect();
    words.sort();
    words.dedup();
    words
}

const WORDS: &str = "ABS AND ANIMFRAME ARG$ ARGC ASC BACKGROUND BITMAPCLEAR BUFFERFETCH BUFFERFREE BUFFERLEN BUFFERNEW CHR$ CIRCLE CLOSE CLS COLOR COLOUR COS CURSOR DATA DEC DEF DIM DO DOUBLEBUFFER DRAWSPRITE DRAWSPRITETILE DRAWTEXT ELLIPSE ELSE ELSEIF END EXIT EXP FILEEXISTS FILLCIRCLE FILLELLIPSE FILLPOLYGON FILLRECT FILLTRIANGLE FLOODFILL FOR FOREACH FUNCTION GET GETBYTE GOSUB GOTO HEX$ HTTP$ HTTPFETCH IF IMAGE INKEY$ INPUT INSTR INT ISMOUSEOVERSPRITE JOY JOYSTICK KEYDOWN KEYPRESS KEYUP LCASE$ LEFT$ LEN LET LINE LOADMUSIC LOADSCREEN LOADSOUND LOADSPRITE LOCATE LOG LOOP MAPLOAD MAPSAVE MID$ MUSICLOOP MUSICPLAYING MUSICVOLUME NEXT NOT ON OPEN OR OVERLAY PALETTESET PAUSEMUSIC PEEK PLATFORM$ PLAYMUSIC PLAYSOUND POKE POLYGON PRINT PSET PUTBYTE READ RECT REM REPLACE RESTORE RESUMEMUSIC RETURN RIGHT$ RND SCREEN SCROLL SGN SIN SLEEP SOUNDPLAYING SPC SPRITE SPRITEAT SPRITECOLLIDE SPRITECOPY SPRITEFRAME SPRITEH SPRITEMODULATE SPRITEVISIBLE SPRITEW SQR STEP STOP STOPMUSIC STOPSOUND STR$ TAB TAN TEXTAT THEN TICKMS TILEMAP TIMER TO TRIANGLE TRIM$ UCASE$ UNLOADMUSIC UNLOADSOUND UNLOADSPRITE UNTIL VAL VSYNC WEND WHILE";

/// File, New's starter: a little of what it can do, line numbers left out as it lets them be.
pub const STARTER: &str = "REM An RGC BASIC program: F9 runs it with RGC BASIC's own interpreter (basic-gfx).
COLOR 1
BACKGROUND 6
CLS
PRINT \"HELLO FROM LIMONITER\"
X = 10
DO
  CLS
  TEXTAT X, 12, \"*\"
  IF KEYDOWN(\"LEFT\") AND X > 0 THEN X = X - 1
  IF KEYDOWN(\"RIGHT\") AND X < 39 THEN X = X + 1
  VSYNC
LOOP
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_starter_is_taken_for_it_and_its_words_are_offered() {
        let lines: Vec<std::sync::Arc<str>> = STARTER.lines().map(std::sync::Arc::from).collect();
        assert_eq!(crate::highlight::guess_basic(&lines), crate::highlight::BASIC_RGC);
        let plain: Vec<std::sync::Arc<str>> = ["10 PRINT \"HELLO\"", "20 GOTO 10"].into_iter().map(std::sync::Arc::from).collect();
        assert_ne!(crate::highlight::guess_basic(&plain), crate::highlight::BASIC_RGC, "an ordinary BASIC isn't it");
        let sprites: Vec<std::sync::Arc<str>> = ["LOADSPRITE 0, \"ship.png\"", "DRAWSPRITE 0, X, Y"].into_iter().map(std::sync::Arc::from).collect();
        assert_eq!(crate::highlight::guess_basic(&sprites), crate::highlight::BASIC_RGC);
        let words = keywords();
        assert!(words.iter().any(|w| w == "LOADSPRITE") && words.iter().any(|w| w == "PLAYSOUND") && words.iter().any(|w| w == "TEXTAT"));
    }
}
