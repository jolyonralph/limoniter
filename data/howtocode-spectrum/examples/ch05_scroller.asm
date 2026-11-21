; ch05_scroller.asm: a scrolling message. Character row 11 moves left a pixel each frame: each of its eight
; pixel lines is shifted through its 32 bytes with RL (HL), right to left, the bit falling off one byte's left
; end going into the next one's right. The new column at the right comes from the ROM's font, a bit at a time.
; Chapter 5, The screen. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

ROW             equ     11

Init:
        xor     a
        call    ClearScreen
        ; Row 11 in bright colours, a different one every four cells.
        ld      hl,$5800+ROW*32
        ld      b,32
        ld      c,0
.colour:
        ld      a,c
        rrca
        rrca
        and     7
        jr      nz,.not_black
        ld      a,7                     ; black ink on black paper would hide the text
.not_black:
        or      %01000000               ; bright
        ld      (hl),a
        inc     hl
        inc     c
        djnz    .colour
        ld      hl,Message
        ld      (Next),hl
        ret

Frame:
        ; A new letter every eight frames: its eight bytes copied from the ROM's font to Letter.
        ld      a,(Column)
        or      a
        jr      nz,.shift
        ld      hl,(Next)
        ld      a,(hl)
        or      a
        jr      nz,.letter
        ld      hl,Message              ; the end: round again
        ld      a,(hl)
.letter:
        inc     hl
        ld      (Next),hl
        ld      l,a
        ld      h,0
        add     hl,hl
        add     hl,hl
        add     hl,hl
        ld      de,$3C00                ; the font is at $3D00, from character 32
        add     hl,de
        ld      de,Letter
        ld      bc,8
        ldir
        ld      a,8
.shift:
        dec     a
        ld      (Column),a
        ; Each pixel line of the row: the letter's next bit into the carry, then the line shifted left through it.
        ld      hl,$4000+(ROW&7)*32+(ROW/8)*2048+31   ; the row's first line, at its right-hand end
        ld      de,Letter
        ld      b,8
.line:
        ld      a,(de)
        rla                             ; the letter's leftmost bit goes in at the right
        ld      (de),a
        inc     de
        ld      c,l                     ; L at the right-hand end, to go back to
        rl      (hl)                    ; 32 bytes, right to left, unrolled: 19 T-states a byte
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)
        dec     l
        rl      (hl)                    ; DEC L doesn't change the carry; the last needs none
        ld      l,c
        inc     h                       ; the next pixel line of the row is 256 bytes on
        djnz    .line
        ret

Interrupt:
        ret

Column:         db      0               ; columns of the letter still to come in
Next:           dw      0               ; the next letter
Letter:         ds      8               ; the letter coming in, shifted left a bit a frame
Message:        db      "HOW TO CODE: THE ZX SPECTRUM ... A PIXEL A FRAME, 32 BYTES A LINE, "
                db      "RL (HL) DOES THE WORK ... GREETINGS TO EVERYONE STILL CODING THE SPECCY!     ",0
