; ch05_multicolour.asm: more than two colours in a character cell. The ULA reads a cell's attribute again for
; every one of its eight lines, so a program that changes the attribute between one line and the next gets eight
; colours a cell. Here twelve cells of eight character rows get a new colour every line, written with PUSH
; (twelve bytes in 76 T-states) while the beam is in the border and retrace, where writing to the screen isn't
; held up, in a loop that takes exactly a scan line. Chapter 5, The screen. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

FIRST_ROW       equ     8               ; character rows 8 to 15: screen lines 64 to 127
LEFT            equ     10              ; columns 10 to 21
; The waits before the first line, in 26 T-states (chapter 5): the loop starts on the line before line 64 of the
; screen, about 100 T-states after the line's first pixel, so its PUSHes fall after the 128 T-states of fetching.
DELAY48         equ     1086
DELAY128        equ     1097

Init:
        ld      a,%00000111
        call    ClearScreen
        ld      hl,Words
        ld      bc,FIRST_ROW*256+LEFT
.row:
        push    bc
        call    PrintAt
        pop     bc
        inc     hl
        inc     b
        ld      a,(hl)
        or      a
        jr      nz,.row
        ld      bc,$1402
        ld      hl,Text
        call    PrintAt
        ld      a,(Is128)
        or      a
        ld      hl,0
        jr      z,.slot
        ld      hl,$0018                ; JR $+2: 12 T-states, where two NOPs take 8
.slot:
        ld      (Slot),hl
        ret

Frame:
        ret

Interrupt:
        ld      a,(Is128)               ; 13
        or      a                       ; 4
        jr      nz,.wait128             ; 7 or 12
        ld      bc,DELAY48              ; 10
        jr      .wait                   ; 12
.wait128:
        ld      bc,DELAY128             ; 10
        nop                             ; 4
.wait:
        dec     bc                      ; 6
        ld      a,b                     ; 4
        or      c                       ; 4
        jr      nz,.wait                ; 12 (7 the last time)
        ld      (SavedSp),sp            ; 20
        ld      a,(Frames)              ; 13
        ld      e,a                     ; 4
        ld      d,Rainbow/256           ; 7
        ld      ix,$5800+FIRST_ROW*32+LEFT+12   ; 14: PUSH writes downwards, from the right-hand cell
        ld      iy,Steps                ; 14
        exx                             ; 4
        ld      b,64                    ; 7: lines
        exx                             ; 4
        ld      b,0                     ; 7: B stays 0, for ADD IX,BC and as DJNZ leaves it
ColourLine:
        ld      a,(de)                  ; 7: this line's colour
        inc     e                       ; 4
        ld      h,a                     ; 4
        ld      l,a                     ; 4
        ld      sp,ix                   ; 10
        push    hl                      ; 11 x 6: twelve attributes
        push    hl
        push    hl
        push    hl
        push    hl
        push    hl
        ld      c,(iy+0)                ; 19: 0, or 32 to the next row
        inc     iy                      ; 10
        add     ix,bc                   ; 15
        ld      b,3                     ; 7
.delay:
        djnz    .delay                  ; 13 x 2 + 8 = 34
        ld      a,(de)                  ; 7, just to wait
        ld      a,(de)                  ; 7
Slot:
        nop                             ; 8 on a 48K, 12 on a 128K
        nop
        exx                             ; 4
        dec     b                       ; 4
        exx                             ; 4
        jp      nz,ColourLine          ; 10: 224 or 228 in all
        ld      sp,(SavedSp)
        ret

SavedSp:        dw      0
Words:          db      "8 colours to",0
                db      "a cell, one",0
                db      "every line,",0
                db      "by PUSH in",0
                db      "the border,",0
                db      "timed to",0
                db      "the T-state",0
                db      "of the line",0
                db      0
Text:           db      "8x1 attributes, 12 cells wide",0

        align   256
; an attribute for each line: bright, the paper changing every two lines, the ink black or white to show on it
Rainbow:
        db      79,79,104,104,120,120,104,104,79,79,71,71,87,87,112,112
        db      120,120,112,112,87,87,71,71,95,95,120,120,95,95,71,71
        db      96,96,112,112,120,120,112,112,96,96,71,71,79,79,104,104
        db      120,120,104,104,79,79,71,71,87,87,112,112,120,120,112,112
        db      87,87,71,71,95,95,120,120,95,95,71,71,96,96,112,112
        db      120,120,112,112,96,96,71,71,79,79,104,104,120,120,104,104
        db      79,79,71,71,87,87,112,112,120,120,112,112,87,87,71,71
        db      95,95,120,120,95,95,71,71,96,96,112,112,120,120,112,112
        db      96,96,71,71,79,79,104,104,120,120,104,104,79,79,71,71
        db      87,87,112,112,120,120,112,112,87,87,71,71,95,95,120,120
        db      95,95,71,71,96,96,112,112,120,120,112,112,96,96,71,71
        db      79,79,104,104,120,120,104,104,79,79,71,71,87,87,112,112
        db      120,120,112,112,87,87,71,71,95,95,120,120,95,95,71,71
        db      96,96,112,112,120,120,112,112,96,96,71,71,79,79,104,104
        db      120,120,104,104,79,79,71,71,87,87,112,112,120,120,112,112
        db      87,87,71,71,95,95,120,120,95,95,71,71,96,96,112,112
; after each line, how far on the next line's attributes are: 32 bytes after every eighth
Steps:
        db      0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,32
        db      0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,32
        db      0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,32
        db      0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,32
