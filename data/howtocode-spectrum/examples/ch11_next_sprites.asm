; ch11_next_sprites.asm: the Next's hardware sprites. One 16 x 16 picture, a byte a pixel ($E3, the sprites'
; see-through colour, outside the ball), is sent to the sprites' pattern memory through port $5B; then each frame
; 32 sprites are placed on a circle that turns, four attribute bytes each through port $57: X, Y, a byte of
; palette offset, mirroring, rotation and X's ninth bit, and one with "visible" and the pattern's number. The
; sprites' coordinates start 32 pixels left of and above the screen's top-left corner. Nextreg $15 turns
; sprites on. For the Next only. Chapter 11, The Next. CC0.

AUTOEXIT        equ     0               ; not used on the Next: this runs until the Next is reset

        device  zxspectrumnext
        org     $8000

NEXTREG_SELECT  equ     $243B
SPRITE_SLOT     equ     $303B           ; which sprite (or pattern) the next writes are for
SPRITE_ATTR     equ     $57
SPRITE_PATTERN  equ     $5B
SPRITES         equ     32

Start:
        di
        nextreg $07,3                   ; 28 MHz
        ; The sprites' first palette (palette control bits 6 to 4 = 010): colour n is RRRGGGBB n.
        nextreg $43,%00100000
        nextreg $40,0
        xor     a
.palette:
        nextreg $41,a
        inc     a
        jr      nz,.palette
        ; Pattern 0: 256 bytes, sent with OTIR (B counts them, from 0: 256; port $5B is all in C).
        ld      bc,SPRITE_SLOT
        xor     a
        out     (c),a
        ld      hl,Ball
        ld      bc,SPRITE_PATTERN       ; B = 0: 256 bytes
        otir
        nextreg $15,%00000001           ; sprites on, over Layer 2 and the ULA (order SLU)
Frame:
        call    WaitFrame
        ld      a,(Turn)
        inc     a
        ld      (Turn),a
        ld      e,a                     ; E: the angle of sprite 0
        ld      bc,SPRITE_SLOT
        xor     a
        out     (c),a                   ; from sprite 0; the attribute writes move on by themselves
        ld      d,SPRITES
.sprite:
        ; x = 32 + 128 - 8 + 100 sin, y = 32 + 96 - 8 + 80 cos: centred on the screen.
        ld      h,Sine/256
        ld      l,e
        ld      a,(hl)                  ; 100 sin, signed
        ld      l,a
        rla
        sbc     a,a
        ld      h,a                     ; HL: sign-extended
        ld      bc,32+128-8
        add     hl,bc                   ; 9-bit x
        ld      a,l
        out     (SPRITE_ATTR),a         ; byte 0: X's low eight bits
        ld      b,h                     ; X's ninth bit, for byte 2
        ld      a,e
        add     a,64                    ; cos is sin a quarter turn on
        ld      h,Sine/256
        ld      l,a
        ld      a,(hl)
        sra     a                       ; 100 x 3/4 = 75, near enough 80 for y
        ld      c,a
        sra     a
        add     a,c
        add     a,32+96-8
        out     (SPRITE_ATTR),a         ; byte 1: Y
        ld      a,b
        and     1
        out     (SPRITE_ATTR),a         ; byte 2: palette offset 0, no mirroring or rotation, X's ninth bit
        ld      a,%10000000
        out     (SPRITE_ATTR),a         ; byte 3: visible, four bytes (bit 6 clear), pattern 0
        ld      a,e
        add     a,256/SPRITES
        ld      e,a
        dec     d
        jr      nz,.sprite
        jr      Frame

; WaitFrame: until the video reaches line 192, then until it's left it (nextreg $1F, the line's low byte).
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

Turn:           db      0

; The ball: 16 lines of 16 colours, RRRGGGBB; $E3 is see-through.
Ball:
        db      $E3,$E3,$E3,$E3,$E3,$E3,$E3,$FC,$FC,$E3,$E3,$E3,$E3,$E3,$E3,$E3
        db      $E3,$E3,$E3,$E3,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$E3,$E3,$E3,$E3
        db      $E3,$E3,$E3,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$E3,$E3,$E3
        db      $E3,$E3,$FC,$FC,$FC,$E0,$E0,$E0,$E0,$E0,$E0,$FC,$FC,$FC,$E3,$E3
        db      $E3,$FC,$FC,$FC,$E0,$E0,$E0,$E0,$E0,$E0,$E0,$E0,$FC,$FC,$FC,$E3
        db      $E3,$FC,$FC,$E0,$E0,$E0,$FF,$FF,$1F,$1F,$E0,$E0,$E0,$FC,$FC,$E3
        db      $E3,$FC,$FC,$E0,$E0,$FF,$FF,$FF,$1F,$1F,$1F,$E0,$E0,$FC,$FC,$E3
        db      $FC,$FC,$FC,$E0,$E0,$FF,$FF,$FF,$1F,$1F,$1F,$E0,$E0,$FC,$FC,$FC
        db      $FC,$FC,$FC,$E0,$E0,$1F,$1F,$1F,$1F,$1F,$1F,$E0,$E0,$FC,$FC,$FC
        db      $E3,$FC,$FC,$E0,$E0,$1F,$1F,$1F,$1F,$1F,$1F,$E0,$E0,$FC,$FC,$E3
        db      $E3,$FC,$FC,$E0,$E0,$E0,$1F,$1F,$1F,$1F,$E0,$E0,$E0,$FC,$FC,$E3
        db      $E3,$FC,$FC,$FC,$E0,$E0,$E0,$E0,$E0,$E0,$E0,$E0,$FC,$FC,$FC,$E3
        db      $E3,$E3,$FC,$FC,$FC,$E0,$E0,$E0,$E0,$E0,$E0,$FC,$FC,$FC,$E3,$E3
        db      $E3,$E3,$E3,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$E3,$E3,$E3
        db      $E3,$E3,$E3,$E3,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$FC,$E3,$E3,$E3,$E3
        db      $E3,$E3,$E3,$E3,$E3,$E3,$E3,$FC,$FC,$E3,$E3,$E3,$E3,$E3,$E3,$E3

; 100 sin(2 pi i / 256), as signed bytes.
        align   256
Sine:
        db      $00,$02,$05,$07,$0A,$0C,$0F,$11,$14,$16,$18,$1B,$1D,$1F,$22,$24
        db      $26,$29,$2B,$2D,$2F,$31,$33,$35,$38,$3A,$3C,$3E,$3F,$41,$43,$45
        db      $47,$48,$4A,$4C,$4D,$4F,$50,$52,$53,$54,$56,$57,$58,$59,$5A,$5B
        db      $5C,$5D,$5E,$5F,$60,$60,$61,$62,$62,$63,$63,$63,$64,$64,$64,$64
        db      $64,$64,$64,$64,$64,$63,$63,$63,$62,$62,$61,$60,$60,$5F,$5E,$5D
        db      $5C,$5B,$5A,$59,$58,$57,$56,$54,$53,$52,$50,$4F,$4D,$4C,$4A,$48
        db      $47,$45,$43,$41,$3F,$3E,$3C,$3A,$38,$35,$33,$31,$2F,$2D,$2B,$29
        db      $26,$24,$22,$1F,$1D,$1B,$18,$16,$14,$11,$0F,$0C,$0A,$07,$05,$02
        db      $00,$FE,$FB,$F9,$F6,$F4,$F1,$EF,$EC,$EA,$E8,$E5,$E3,$E1,$DE,$DC
        db      $DA,$D7,$D5,$D3,$D1,$CF,$CD,$CB,$C8,$C6,$C4,$C2,$C1,$BF,$BD,$BB
        db      $B9,$B8,$B6,$B4,$B3,$B1,$B0,$AE,$AD,$AC,$AA,$A9,$A8,$A7,$A6,$A5
        db      $A4,$A3,$A2,$A1,$A0,$A0,$9F,$9E,$9E,$9D,$9D,$9D,$9C,$9C,$9C,$9C
        db      $9C,$9C,$9C,$9C,$9C,$9D,$9D,$9D,$9E,$9E,$9F,$A0,$A0,$A1,$A2,$A3
        db      $A4,$A5,$A6,$A7,$A8,$A9,$AA,$AC,$AD,$AE,$B0,$B1,$B3,$B4,$B6,$B8
        db      $B9,$BB,$BD,$BF,$C1,$C2,$C4,$C6,$C8,$CB,$CD,$CF,$D1,$D3,$D5,$D7
        db      $DA,$DC,$DE,$E1,$E3,$E5,$E8,$EA,$EC,$EF,$F1,$F4,$F6,$F9,$FB,$FE
