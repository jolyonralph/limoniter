; ch13_tape.asm: a tape made the way a release's is, by the source itself with SAVETAP. Three files: a BASIC
; loader that runs itself from line 10, a loading screen (LOAD "" SCREEN$, 6,912 bytes at 16384), and the code.
; The loader is written here byte by byte, as the Spectrum keeps a BASIC line: its number (high byte first), its
; length, the keywords as single-byte tokens, and ENTER. F9 runs the tape the source saved, not one of its own.
; Once loaded, the program keeps the screen and turns its colours round. The screen is made by
; ch13_make_screen.py. Chapter 13, Releasing. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

Init:
        ret

; Each frame, every attribute's ink moved on one colour (1 to 7, round again), so the stripes seem to move.
Frame:
        ld      a,(Frames)
        and     3
        ret     nz
        ld      hl,$5800
        ld      bc,768
.cell:
        ld      a,(hl)
        and     7                       ; the ink
        inc     a
        cp      8                       ; after 7, back to 1 (0 would be black on black)
        jr      c,.keep
        ld      a,1
.keep:
        or      %01000000               ; bright, on black paper
        ld      (hl),a
        inc     hl
        dec     bc
        ld      a,b
        or      c
        jr      nz,.cell
        ret

Interrupt:
        ret
CodeEnd:

; The BASIC loader, as it's kept in memory, put here only to be saved: it's never run from here.
; 10 CLEAR VAL "32767": LOAD ""SCREEN$ : LOAD ""CODE : RANDOMIZE USR VAL "32768"
Loader:
        db      0,10                    ; line 10, high byte first
        dw      LineEnd-LineStart       ; its length, low byte first
LineStart:
        db      $FD,$B0,'"32767":'      ; CLEAR VAL "32767":
        db      $EF,'""',$AA,':'        ; LOAD ""SCREEN$ :
        db      $EF,'""',$AF,':'        ; LOAD ""CODE :
        db      $F9,$C0,$B0,'"32768"'   ; RANDOMIZE USR VAL "32768"
        db      $0D                     ; ENTER ends the line
LineEnd:
LoaderEnd:

; The loading screen, in the screen's own memory so SAVETAP can save it from there.
        org     $4000
Screen:
        incbin  "ch13_screen.scr"

; The tape: the loader (to run from line 10), the screen, and the code, each with its header.
        savetap "ch13_tape.tap",BASIC,"HowToCode",Loader,LoaderEnd-Loader,10
        savetap "ch13_tape.tap",CODE,"screen",$4000,6912
        savetap "ch13_tape.tap",CODE,"code",$8000,CodeEnd-$8000
