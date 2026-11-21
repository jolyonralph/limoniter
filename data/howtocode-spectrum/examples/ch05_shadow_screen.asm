; ch05_shadow_screen.asm: double buffering on the 128K's shadow screen. Bank 7 holds a second screen, shown in
; place of bank 5's when bit 3 of port $7FFD is set. Each frame the middle third of the screen that isn't being
; shown is cleared and a bar drawn on it, then the two are swapped as the next frame starts: what's shown is
; always finished, so nothing flickers, however long the drawing takes. 128K, +2, +2A and +3 only.
; Chapter 5, The screen; chapter 8, The 128K and later models. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum128
        include "startup.asm"

Init:
        ; Bank 7 at $C000 for the whole run, so both screens can be written: bank 5's at $4000, bank 7's at $C000.
        ld      a,(BANKM)
        and     %00010000               ; the ROM BASIC had
        or      7
        ld      (Port),a
        ld      bc,$7FFD
        out     (c),a
        ld      a,%00101000             ; black on cyan
        call    ClearScreen
        ld      hl,$4000                ; and the same on bank 7's screen
        ld      de,$C000
        ld      bc,6912
        ldir
        ld      a,$C0                   ; draw first on the screen that isn't shown: bank 7's, at $C000
        ld      (Drawing),a
        ld      hl,Text                 ; the same words on both, at the top
        ld      bc,$0101
        call    PrintAt
        ld      hl,$4000
        ld      de,$C000
        ld      bc,2048
        ldir
        ret

Frame:
        ; Clear the middle third (character rows 8 to 15: 2048 bytes) of the screen being drawn on.
        ld      a,(Drawing)
        add     a,8                     ; the middle third is 2048 bytes on
        ld      h,a
        ld      l,0
        ld      d,h
        ld      e,1
        ld      (hl),0
        ld      bc,2047
        ldir
        ; A bar four bytes wide, all 64 lines of the third, at a column that moves.
        ld      a,(Frames)
        and     31
        cp      29
        jr      c,.place
        sub     28
.place:
        ld      e,a                     ; the column
        ld      a,(Drawing)
        add     a,8
        ld      h,a
        ld      l,0
        ld      b,64
.line:
        push    hl
        ld      a,l
        and     %11100000               ; the line's start, plus the column
        or      e
        ld      l,a
        ld      a,$FF
        ld      (hl),a
        inc     l
        ld      (hl),a
        inc     l
        ld      (hl),a
        inc     l
        ld      (hl),a
        pop     hl
        call    LineDown
        djnz    .line
        ; Done: show it as the next frame starts, and draw on the other next time.
        ld      a,1
        ld      (Swap),a
        ret

; LineDown: HL down one pixel line on the screen (see chapter 5). Works on either screen: only H's low bits and
; L change.
LineDown:
        inc     h
        ld      a,h
        and     7
        ret     nz
        ld      a,l
        add     a,32
        ld      l,a
        ret     c
        ld      a,h
        sub     8
        ld      h,a
        ret

; At the start of the frame, before the ULA reaches the screen: swap them if a new picture is finished.
Interrupt:
        ld      a,(Swap)
        or      a
        ret     z
        xor     a
        ld      (Swap),a
        ld      a,(Port)
        xor     %00001000               ; bit 3: the shadow screen shown, or not
        ld      (Port),a
        ld      bc,$7FFD
        out     (c),a
        ld      a,(Drawing)
        xor     $40 ^ $C0               ; and draw on the other one
        ld      (Drawing),a
        ret

Port:           db      0               ; what was last written to $7FFD
Drawing:        db      0               ; the high byte of the screen being drawn on: $40 or $C0
Swap:           db      0
Text:           db      "Drawn on one, showing the other",0
