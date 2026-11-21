; ch06_sprites.asm: masked, pre-shifted sprites. Three balls bounce over a patterned background. Each is drawn
; with its mask (AND) and then its picture (OR), so the background shows round it; the eight ways it can sit
; across a byte are shifted once at the start, not every frame; and what each covers is saved before it's drawn
; and put back before it moves. Chapter 6, Sprites. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

SPRITES         equ     3
SHIFTED_SIZE    equ     16*6            ; a shifted copy: 16 lines of mask, picture, mask, picture, mask, picture

Init:
        ld      a,%00111000
        call    ClearScreen
        ; The background: alternate lines of $AA and $55, a fine checkerboard, in bands of colour.
        ld      hl,$4000
        ld      bc,6144
.pattern:
        ld      a,h
        and     1
        ld      a,$AA
        jr      z,.even
        ld      a,$55
.even:
        ld      (hl),a
        inc     hl
        dec     bc
        ld      a,b
        or      c
        jr      nz,.pattern
        ld      hl,$5800
        ld      bc,768
.attributes:
        ld      a,l
        rrca
        rrca
        and     %00000111
        or      %00111000               ; white paper, ink from the column
        ld      (hl),a
        inc     hl
        dec     bc
        ld      a,b
        or      c
        jr      nz,.attributes
        call    PreShift
        ; Each sprite starts by saving what's under it, so the first frame's putting back has something to put.
        ld      ix,Sprites
        ld      b,SPRITES
.first:
        push    bc
        call    Save
        ld      de,SPRITE
        add     ix,de
        pop     bc
        djnz    .first
        ret

; PreShift: Ball's 16 lines, two bytes wide, made into eight copies three bytes wide, shifted right 0 to 7
; pixels. The picture's new bits are 0; the mask's are 1, so the background shows through them.
PreShift:
        ld      de,Shifted
        ld      c,0                     ; the shift
.copy:
        ld      hl,Ball
        ld      b,16
.line:
        push    bc
        ld      ix,Line
        ld      a,(hl)
        ld      (ix+0),a
        inc     hl
        ld      a,(hl)
        ld      (ix+1),a
        inc     hl
        ld      (ix+2),$FF
        ld      a,(hl)
        ld      (ix+3),a
        inc     hl
        ld      a,(hl)
        ld      (ix+4),a
        inc     hl
        ld      (ix+5),0
        ld      a,c
        or      a
        jr      z,.shifted
        ld      b,a
.shift:
        scf                             ; a 1 into the mask
        rr      (ix+0)
        rr      (ix+1)
        rr      (ix+2)
        or      a                       ; a 0 into the picture
        rr      (ix+3)
        rr      (ix+4)
        rr      (ix+5)
        djnz    .shift
.shifted:
        ld      a,(ix+0)                ; mask and picture, a byte of each in turn
        ld      (de),a
        inc     de
        ld      a,(ix+3)
        ld      (de),a
        inc     de
        ld      a,(ix+1)
        ld      (de),a
        inc     de
        ld      a,(ix+4)
        ld      (de),a
        inc     de
        ld      a,(ix+2)
        ld      (de),a
        inc     de
        ld      a,(ix+5)
        ld      (de),a
        inc     de
        pop     bc
        djnz    .line
        inc     c
        ld      a,c
        cp      8
        jr      nz,.copy
        ret

Frame:
        ; Put back what each covered, the last drawn first, so where two overlap the background comes back right.
        ld      ix,Sprites+(SPRITES-1)*SPRITE
        ld      b,SPRITES
.restore:
        push    bc
        call    Restore
        ld      de,-SPRITE
        add     ix,de
        pop     bc
        djnz    .restore
        ; Move each, save what's under it, and draw it.
        ld      ix,Sprites
        ld      b,SPRITES
.draw:
        push    bc
        call    Move
        call    Save
        call    Draw
        ld      de,SPRITE
        add     ix,de
        pop     bc
        djnz    .draw
        ret

; A sprite, at IX: x, y, how far it moves each frame in x and in y, the screen address it was saved from, and
; the 48 bytes it covered.
SX              equ     0
SY              equ     1
DX              equ     2
DY              equ     3
SAVED_AT        equ     4
SAVED           equ     6
SPRITE          equ     6+48

Move:
        ld      a,(ix+SX)
        add     a,(ix+DX)
        cp      256-24                  ; three bytes wide must fit: x from 0 to 232
        jr      c,.x_in
        ld      a,(ix+DX)
        neg
        ld      (ix+DX),a
        ld      a,(ix+SX)
.x_in:
        ld      (ix+SX),a
        ld      a,(ix+SY)
        add     a,(ix+DY)
        cp      192-16
        jr      c,.y_in
        ld      a,(ix+DY)
        neg
        ld      (ix+DY),a
        ld      a,(ix+SY)
.y_in:
        ld      (ix+SY),a
        ret

; Save: the three bytes by sixteen lines under the sprite, and where they are.
Save:
        ld      b,(ix+SY)
        ld      c,(ix+SX)
        call    PixelAddress
        ld      (ix+SAVED_AT),l
        ld      (ix+SAVED_AT+1),h
        push    ix
        pop     de
        ld      a,e
        add     a,SAVED
        ld      e,a
        adc     a,d
        sub     e
        ld      d,a                     ; DE: IX + SAVED
        ld      b,16
.line:
        push    hl
        ld      a,(hl)
        ld      (de),a
        inc     de
        inc     l
        ld      a,(hl)
        ld      (de),a
        inc     de
        inc     l
        ld      a,(hl)
        ld      (de),a
        inc     de
        pop     hl
        call    LineDown
        djnz    .line
        ret

Restore:
        ld      l,(ix+SAVED_AT)
        ld      h,(ix+SAVED_AT+1)
        push    ix
        pop     de
        ld      a,e
        add     a,SAVED
        ld      e,a
        adc     a,d
        sub     e
        ld      d,a
        ld      b,16
.line:
        push    hl
        ld      a,(de)
        ld      (hl),a
        inc     de
        inc     l
        ld      a,(de)
        ld      (hl),a
        inc     de
        inc     l
        ld      a,(de)
        ld      (hl),a
        inc     de
        pop     hl
        call    LineDown
        djnz    .line
        ret

; Draw: the copy shifted by x AND 7, masked onto the screen at the address Save found.
Draw:
        ld      a,(ix+SX)
        and     7
        ld      l,a
        ld      h,0
        ld      d,h
        ld      e,l
        add     hl,hl                   ; x 96: x 3, then x 32
        add     hl,de
        add     hl,hl
        add     hl,hl
        add     hl,hl
        add     hl,hl
        add     hl,hl
        ld      de,Shifted
        add     hl,de
        ex      de,hl                   ; DE: the shifted copy
        ld      l,(ix+SAVED_AT)
        ld      h,(ix+SAVED_AT+1)
        ld      b,16
.line:
        push    hl
        ld      a,(de)                  ; mask
        and     (hl)
        inc     de
        ld      c,a
        ld      a,(de)                  ; picture
        or      c
        ld      (hl),a
        inc     de
        inc     l
        ld      a,(de)
        and     (hl)
        inc     de
        ld      c,a
        ld      a,(de)
        or      c
        ld      (hl),a
        inc     de
        inc     l
        ld      a,(de)
        and     (hl)
        inc     de
        ld      c,a
        ld      a,(de)
        or      c
        ld      (hl),a
        inc     de
        pop     hl
        call    LineDown
        djnz    .line
        ret

; LineDown: HL down one pixel line on the screen (see chapter 5).
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

Interrupt:
        ret

; The ball: for each of its 16 lines, two bytes of mask (1 where the background shows), then two of picture.
Ball:
        db      %11111100,%00111111, %00000000,%00000000
        db      %11110000,%00001111, %00000000,%10000000
        db      %11000000,%00000011, %00000101,%01010000
        db      %11000000,%00000011, %00001010,%10101000
        db      %10000000,%00000001, %00010001,%11110100
        db      %10000000,%00000001, %00100001,%11111000
        db      %00000000,%00000000, %00010001,%11110100
        db      %00000000,%00000000, %00101111,%11111010
        db      %00000000,%00000000, %01011111,%11110100
        db      %00000000,%00000000, %00101111,%11111000
        db      %10000000,%00000001, %00011111,%11110100
        db      %10000000,%00000001, %00101111,%11101000
        db      %11000000,%00000011, %00010101,%01010000
        db      %11000000,%00000011, %00001010,%10100000
        db      %11110000,%00001111, %00000001,%00000000
        db      %11111100,%00111111, %00000000,%00000000

Sprites:        db      20,30,2,1, 0,0
                ds      48
                db      120,100,-1,2, 0,0
                ds      48
                db      200,60,3,-1, 0,0
                ds      48
Line:           ds      6               ; a line being shifted: three bytes of mask, three of picture
Shifted:        ds      8*SHIFTED_SIZE
