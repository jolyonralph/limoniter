; startup_demo.asm: the smallest whole demo on How To Code's Spectrum startup. A title, and a band of colour
; that rolls along it, changed in Frame; the border flashes in Interrupt. SPACE goes back to BASIC.
; Chapter 4, Startup and interrupts. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

Init:
        ld      a,%00111000             ; black ink on white paper
        call    ClearScreen
        ld      bc,$0A02                ; row 10, column 2
        ld      hl,Title
        call    PrintAt
        ld      bc,$0D05
        ld      hl,Press
        jp      PrintAt

; Each frame, row 10's 32 attributes are given the colours of a ring of eight, one step further on.
Frame:
        ld      a,(Frames)
        ld      c,a
        ld      hl,$5800+10*32
        ld      b,32
.cell:
        ld      a,c
        inc     c
        rrca                            ; a new colour every two cells
        and     7
        ld      e,a
        ld      d,0
        push    hl
        ld      hl,Colours
        add     hl,de
        ld      a,(hl)
        pop     hl
        ld      (hl),a
        inc     hl
        djnz    .cell
        ret

; The border, black and blue in turn every 16 frames.
Interrupt:
        ld      a,(Frames)
        and     16
        rrca
        rrca
        rrca
        rrca
        out     ($FE),a
        ret

Colours:        db      %01000111,%01000110,%01000101,%01000100,%01000011,%01000010,%01000001,%01111000
Title:          db      "How To Code: the ZX Spectrum",0
Press:          db      "SPACE goes back",0
