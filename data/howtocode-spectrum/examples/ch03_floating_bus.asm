; ch03_floating_bus.asm: catching the beam with the floating bus. Row 12 is given an attribute no other byte on
; the screen has. Each frame, port $FF (which nothing answers) is read until that byte goes past on the data bus,
; as the ULA fetches it to draw row 12; the border turns red there for eight lines. On a 48K or 128K the red band
; sits beside row 12, steady. On a +2A or +3, which have no floating bus on port $FF, it never comes, and the
; text says so. Chapter 3, The machine. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

MARKER          equ     %01010110       ; bright, red paper, yellow ink: used nowhere else on the screen
ROW             equ     12

Init:
        ld      a,%00000111             ; white ink on black paper, the pixels all clear
        call    ClearScreen
        ld      hl,$5800+ROW*32
        ld      b,32
.row:
        ld      (hl),MARKER
        inc     hl
        djnz    .row
        ; The text goes below row 12, so none of its bytes is read before the marker's.
        ld      bc,$1001
        ld      hl,Text
        call    PrintAt
        xor     a
        out     ($FE),a
        ret

Frame:
        ld      hl,Frames
        ld      e,(hl)
        ld      bc,$00FF                ; port $FF, with a high byte of 0, which no device takes as its own
.poll:
        in      a,(c)                   ; 12 T-states
        cp      MARKER                  ; 7
        jr      z,.found                ; 7
        ld      a,(hl)                  ; 7: a frame gone by with no marker means there's no floating bus
        cp      e                       ; 4
        jr      z,.poll                 ; 12: 49 in all, an odd number (see chapter 3)
        ld      hl,NotFound
        jr      .say
.found:
        ld      a,2                     ; red, at once
        out     ($FE),a
        ld      bc,69                   ; eight lines: 69 times round, 26 T-states each, about 1800 T-states
.wait:
        dec     bc
        ld      a,b
        or      c
        jr      nz,.wait
        xor     a
        out     ($FE),a
        ld      hl,Found
.say:
        ld      bc,$1201
        jp      PrintAt

Interrupt:                              ; nothing to do in the interrupt
        ret

Text:           db      "Port $FF read until row 12 goes",0
Found:          db      "The floating bus: found it.    ",0
NotFound:       db      "No floating bus on port $FF.   ",0
