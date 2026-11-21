; ch11_next_layer2.asm: the Spectrum Next's Layer 2, a 256 x 192 screen of one byte a pixel, each byte a colour
; from a 256-colour palette. The program sets the CPU to 28 MHz, fills the palette so that colour n is the 8-bit
; RRRGGGBB value n, draws x XOR y into Layer 2's six 8K pages (paged in one at a time at $C000 by MMU slot 6),
; turns Layer 2 on, and then scrolls it by hardware, a pixel a frame each way. Colour $E3 is see-through
; (nextreg $14), so the black of the ULA's screen behind shows in its few pixels of that colour. It waits for each
; frame by reading the line the video is on (nextreg $1F), so it needs no interrupts, and runs until the Next is
; reset.
; For the Next only: it doesn't use the startup. Chapter 11, The Next. CC0.

AUTOEXIT        equ     0               ; not used on the Next: this runs until the Next is reset

        device  zxspectrumnext
        org     $8000

NEXTREG_SELECT  equ     $243B
NEXTREG_DATA    equ     $253B
LAYER2_PORT     equ     $123B

Start:
        di
        nextreg $07,3                   ; 28 MHz
        ; Layer 2's palette: the first one (palette control $43, bits 6 to 4 = 001), index 0 up, n = n.
        nextreg $43,%00010000
        nextreg $40,0
        xor     a
.palette:
        nextreg $41,a                   ; the index moves on by itself after each write
        inc     a
        jr      nz,.palette
        ; Which 16K bank Layer 2 is shown from (nextreg $12): its 8K pages are that bank x 2, and the five after.
        ld      bc,NEXTREG_SELECT
        ld      a,$12
        out     (c),a
        inc     b                       ; $253B
        in      a,(c)
        add     a,a
        ld      e,a                     ; E: the first 8K page
        ld      d,0                     ; D: y at the top of this page (32 lines a page)
        ld      b,6
.page:
        push    bc
        ld      a,e
        nextreg $56,a                   ; MMU slot 6 ($C000 to $DFFF): this page
        ld      hl,$C000
.line:
        ld      a,h
        sub     $C0
        add     a,d                     ; y
        ld      c,a
.pixel:
        ld      a,l                     ; x
        xor     c
        ld      (hl),a
        inc     l
        jr      nz,.pixel
        inc     h
        ld      a,h
        cp      $E0
        jr      nz,.line
        ld      a,d
        add     a,32
        ld      d,a
        inc     e
        pop     bc
        djnz    .page
        nextreg $56,0                   ; slot 6 back to bank 0's first page, as the Next started
        ld      bc,LAYER2_PORT
        ld      a,%00000010             ; bit 1: Layer 2 shown
        out     (c),a
Frame:
        call    WaitFrame
        ld      a,(ScrollX)
        inc     a
        ld      (ScrollX),a
        nextreg $16,a                   ; X scroll, 0 to 255
        ld      a,(ScrollY)
        inc     a
        cp      192                     ; Y scroll is 0 to 191
        jr      c,.y
        xor     a
.y:
        ld      (ScrollY),a
        nextreg $17,a
        jr      Frame

; WaitFrame: waits until the video reaches line 192, just past the bottom of the picture (nextreg $1E and $1F
; hold the line; 192 is only ever the low byte with a high byte of 0), and then until it's left it.
WaitFrame:
        ld      bc,NEXTREG_SELECT
        ld      a,$1F
        out     (c),a
        inc     b
.wait:
        in      a,(c)
        cp      192
        jr      nz,.wait
.leave:
        in      a,(c)
        cp      192
        jr      z,.leave
        ret

ScrollX:        db      0
ScrollY:        db      0
