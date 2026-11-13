; Rainbow stripes in the attributes, moving until a key is pressed.
; Attribute bytes: bit 7 FLASH, bit 6 BRIGHT, bits 5-3 PAPER, bits 2-0 INK.

        org 32768
start:  xor a
        out (254),a         ; black border
        ld c,0              ; C: how far the stripes have moved, 0 to 5

frame:  halt                ; wait for the next frame
        ld hl,22528         ; the attributes: 24 rows of 32 cells from 22528
        ld b,c              ; B: the colour this row starts with, 0 to 5
        ld d,24             ; D: rows to go

row:    push bc
        ld e,32             ; E: cells to go in this row
cell:   push hl
        ld hl,colours       ; look up colour B
        ld a,l
        add a,b
        ld l,a              ; (the table doesn't cross a 256-byte page)
        ld a,(hl)
        pop hl
        ld (hl),a           ; colour this cell
        inc hl
        inc b               ; the next cell is the next colour
        ld a,b
        cp 6
        jr nz,same
        ld b,0
same:   dec e
        jr nz,cell

        pop bc
        inc b               ; each row starts one colour on: diagonal stripes
        ld a,b
        cp 6
        jr nz,down
        ld b,0
down:   dec d
        jr nz,row

        inc c               ; move the stripes along for the next frame
        ld a,c
        cp 6
        jr nz,keys
        ld c,0
keys:   xor a
        in a,(254)          ; read every row of the keyboard at once
        cpl
        and 31              ; is any key down?
        jr z,frame
        ret                 ; back to BASIC

        align 8
; Bright papers in rainbow order: red, yellow, green, cyan, blue, magenta.
colours: db 64+2*8, 64+6*8, 64+4*8, 64+5*8, 64+1*8, 64+3*8
