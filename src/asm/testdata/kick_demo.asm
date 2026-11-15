// A raster bar and a scroller's skeleton, in KickAssembler's way, to check Limoniter's
// assembler makes what KickAssembler makes.
.const BORDER = $d020
.const SCREEN = $d021
.label screen = $0400
.var speed = 2

BasicUpstart2(start)

* = $0810 "Code"
start:
        sei
        lda #$7f
        sta $dc0d
        sta $dd0d
        lda #$35
        sta $01
        lda #<irq
        sta $fffe
        lda #>irq
        sta $ffff
        lda #$1b
        sta $d011
        lda #$80
        sta $d012
        lda #$01
        sta $d01a
        lda $dc0d
        asl $d019
        cli
        jsr clear
!loop:
        jmp !loop-

clear: {
        ldx #0
        lda #' '
loop:   sta screen,x
        sta screen+$100,x
        sta screen+$200,x
        sta screen+$300,x
        inx
        bne loop
        rts
}

irq: {
        pha
        txa
        pha
        ldx #0
!:      lda colours,x
        sta BORDER
        sta SCREEN
        ldy #8
!:      dey
        bne !-
        inx
        cpx #colours_end-colours
        bne !--
        lda #BLACK
        sta BORDER
        sta SCREEN
        asl $d019
        pla
        tax
        pla
        rti
}

.macro Wait(cycles) {
        ldx #cycles
!:      dex
        bne !-
}

        :Wait(speed)
        Wait(10)

colours:
.for (var i=0; i<8; i++) {
        .byte i
}
        .byte RED, ORANGE, YELLOW, WHITE
colours_end:
message:
        .text "hello, kickassembler"
        .byte 0
        .word start, irq, clear.loop
        .fill 4, $ea
        .align $10
table:  .byte <table, >table
        lda modify:#$00
        sta modify
        .pseudopc $c000 {
moved:  jmp moved
        }
.if (speed == 2) {
        nop
} else {
        brk
}
        .encoding "petscii_mixed"
        .text "Mixed Case"
        .encoding "petscii_upper"
        .text "upper"
        .encoding "screencode_upper"
        .text "upper"
