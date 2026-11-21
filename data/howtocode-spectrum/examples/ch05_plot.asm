; ch05_plot.asm: plotting fast with lookup tables. 64 dots on a Lissajous curve, each frame rubbed out and drawn
; again with XOR. A table of the 192 lines' addresses (made at the start by moving down a line at a time) turns
; y into an address with two loads, and a table of the eight bits turns x into the pixel's bit.
; Chapter 5, The screen. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

DOTS            equ     64

Init:
        ld      a,%01000101             ; bright cyan ink on black
        call    ClearScreen
        ; The line table: the address of x = 0 on each line y, its low bytes in one 256-byte page and its high bytes
        ; in the next, so that with H the page and L = y, (HL) is the low byte and, after INC H, the high.
        ld      hl,$4000
        ld      de,LineLow
        ld      b,192
.line:
        ld      a,l
        ld      (de),a
        inc     d
        ld      a,h
        ld      (de),a
        dec     d
        inc     e
        call    LineDown
        djnz    .line
        ; Nothing to rub out on the first frame: the list of plotted dots points at the ROM, where writes do nothing.
        ld      hl,Plotted
        ld      b,DOTS*3
.clear:
        ld      (hl),0
        inc     hl
        djnz    .clear
        ret

; LineDown: HL, an address on the screen, moved down one pixel line. Within a character row that's the next
; 256 bytes; past the last line of a row, it's the next row, 32 bytes on, back at the row's first line; past the
; last row of a third of the screen, it's the next third.
LineDown:
        inc     h
        ld      a,h
        and     7
        ret     nz
        ld      a,l
        add     a,32
        ld      l,a
        ret     c                       ; into the next third: H has already moved on by 8
        ld      a,h
        sub     8
        ld      h,a
        ret

Frame:
        ; Rub out the last frame's dots: XOR each byte with its bit again.
        ld      ix,Plotted
        ld      b,DOTS
.rub:
        ld      l,(ix+0)
        ld      h,(ix+1)
        ld      a,(ix+2)
        xor     (hl)
        ld      (hl),a
        inc     ix
        inc     ix
        inc     ix
        djnz    .rub
        ; And plot the new ones, keeping each one's address and bit.
        ld      ix,Plotted
        ld      a,(Frames)
        ld      e,a                     ; E: where along the curve dot 0 is, in x
        add     a,a
        ld      d,a                     ; D: and in y, twice as fast
        ld      b,DOTS
.dot:
        push    bc
        ld      h,XSine/256
        ld      l,e
        ld      c,(hl)                  ; x
        ld      h,YSine/256
        ld      l,d
        ld      b,(hl)                  ; y
        call    Plot
        pop     bc
        ld      a,e
        add     a,4*3                   ; the dots spread round the curve: 3 x cycles, 2 y cycles, a Lissajous figure
        ld      e,a
        ld      a,d
        add     a,4*2
        ld      d,a
        djnz    .dot
        ret

; Plot: the pixel at x = C, y = B XORed onto the screen, its address and bit kept at IX, which moves on.
Plot:
        ld      h,LineLow/256
        ld      l,b
        ld      a,c
        rrca
        rrca
        rrca
        and     31                      ; the column, x / 8
        or      (hl)                    ; the line's low byte has 0 in its column bits
        inc     h
        ld      h,(hl)                  ; L is still y: the high byte
        ld      l,a
        ld      a,c
        and     7
        push    hl
        ld      h,Bits/256
        add     a,Bits&255
        ld      l,a
        ld      a,(hl)
        pop     hl
        ld      (ix+0),l
        ld      (ix+1),h
        ld      (ix+2),a
        inc     ix
        inc     ix
        inc     ix
        xor     (hl)
        ld      (hl),a
        ret

Interrupt:
        ret

        align   256
LineLow:        ds      256             ; filled by Init: the low bytes (only 192 used)
LineHigh:       ds      256             ; and the high bytes, in the next page
; 128 + 100 sin(2 pi i / 256), for x
XSine:
        db      128,130,133,135,138,140,143,145,148,150,152,155,157,159,162,164
        db      166,169,171,173,175,177,179,181,184,186,188,190,191,193,195,197
        db      199,200,202,204,205,207,208,210,211,212,214,215,216,217,218,219
        db      220,221,222,223,224,224,225,226,226,227,227,227,228,228,228,228
        db      228,228,228,228,228,227,227,227,226,226,225,224,224,223,222,221
        db      220,219,218,217,216,215,214,212,211,210,208,207,205,204,202,200
        db      199,197,195,193,191,190,188,186,184,181,179,177,175,173,171,169
        db      166,164,162,159,157,155,152,150,148,145,143,140,138,135,133,130
        db      128,126,123,121,118,116,113,111,108,106,104,101,99,97,94,92
        db      90,87,85,83,81,79,77,75,72,70,68,66,65,63,61,59
        db      57,56,54,52,51,49,48,46,45,44,42,41,40,39,38,37
        db      36,35,34,33,32,32,31,30,30,29,29,29,28,28,28,28
        db      28,28,28,28,28,29,29,29,30,30,31,32,32,33,34,35
        db      36,37,38,39,40,41,42,44,45,46,48,49,51,52,54,56
        db      57,59,61,63,65,66,68,70,72,75,77,79,81,83,85,87
        db      90,92,94,97,99,101,104,106,108,111,113,116,118,121,123,126
; 96 + 80 sin(2 pi i / 256), for y
YSine:
        db      96,98,100,102,104,106,108,110,112,114,115,117,119,121,123,125
        db      127,128,130,132,134,135,137,139,140,142,144,145,147,148,150,151
        db      153,154,155,157,158,159,160,161,163,164,165,166,167,167,168,169
        db      170,171,171,172,173,173,174,174,174,175,175,175,176,176,176,176
        db      176,176,176,176,176,175,175,175,174,174,174,173,173,172,171,171
        db      170,169,168,167,167,166,165,164,163,161,160,159,158,157,155,154
        db      153,151,150,148,147,145,144,142,140,139,137,135,134,132,130,128
        db      127,125,123,121,119,117,115,114,112,110,108,106,104,102,100,98
        db      96,94,92,90,88,86,84,82,80,78,77,75,73,71,69,67
        db      65,64,62,60,58,57,55,53,52,50,48,47,45,44,42,41
        db      39,38,37,35,34,33,32,31,29,28,27,26,25,25,24,23
        db      22,21,21,20,19,19,18,18,18,17,17,17,16,16,16,16
        db      16,16,16,16,16,17,17,17,18,18,18,19,19,20,21,21
        db      22,23,24,25,25,26,27,28,29,31,32,33,34,35,37,38
        db      39,41,42,44,45,47,48,50,52,53,55,57,58,60,62,64
        db      65,67,69,71,73,75,77,78,80,82,84,86,88,90,92,94
Bits:           db      $80,$40,$20,$10,$08,$04,$02,$01
Plotted:        ds      DOTS*3          ; each dot plotted: its address and its bit
