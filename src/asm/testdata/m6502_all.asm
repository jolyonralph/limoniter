!cpu 6510
*=$1000
start:
  adc #$12
  adc $34
  adc $34,x
  adc $1234
  adc $1234,x
  adc $1234,y
  adc ($34,x)
  adc ($34),y
  and #$12
  and $34
  and $34,x
  and $1234
  and $1234,x
  and $1234,y
  and ($34,x)
  and ($34),y
  asl
  asl $34
  asl $34,x
  asl $1234
  asl $1234,x
  bcc start
  bcs start
  beq start
  bmi start
  bne start
  bpl start
  bvc start
  bvs start
  bit $34
  bit $1234
  brk
  clc
  cld
  cli
  clv
  cmp #$12
  cmp $34
  cmp $34,x
  cmp $1234
  cmp $1234,x
  cmp $1234,y
  cmp ($34,x)
  cmp ($34),y
  cpx #$12
  cpx $34
  cpx $1234
  cpy #$12
  cpy $34
  cpy $1234
  dec $34
  dec $34,x
  dec $1234
  dec $1234,x
  dex
  dey
  eor #$12
  eor $34
  eor $34,x
  eor $1234
  eor $1234,x
  eor $1234,y
  eor ($34,x)
  eor ($34),y
  inc $34
  inc $34,x
  inc $1234
  inc $1234,x
  inx
  iny
  jmp $1234
  jmp ($1234)
  jsr $1234
  lda #$12
  lda $34
  lda $34,x
  lda $1234
  lda $1234,x
  lda $1234,y
  lda ($34,x)
  lda ($34),y
  ldx #$12
  ldx $34
  ldx $34,y
  ldx $1234
  ldx $1234,y
  ldy #$12
  ldy $34
  ldy $34,x
  ldy $1234
  ldy $1234,x
  lsr
  lsr $34
  lsr $34,x
  lsr $1234
  lsr $1234,x
  nop
  ora #$12
  ora $34
  ora $34,x
  ora $1234
  ora $1234,x
  ora $1234,y
  ora ($34,x)
  ora ($34),y
  pha
  php
  pla
  plp
  rol
  rol $34
  rol $34,x
  rol $1234
  rol $1234,x
  ror
  ror $34
  ror $34,x
  ror $1234
  ror $1234,x
  rti
  rts
  sbc #$12
  sbc $34
  sbc $34,x
  sbc $1234
  sbc $1234,x
  sbc $1234,y
  sbc ($34,x)
  sbc ($34),y
  sec
  sed
  sei
  sta $34
  sta $34,x
  sta $1234
  sta $1234,x
  sta $1234,y
  sta ($34,x)
  sta ($34),y
  stx $34
  stx $34,y
  stx $1234
  sty $34
  sty $34,x
  sty $1234
  tax
  tay
  tsx
  txa
  txs
  tya
  slo $34
  slo $34,x
  slo $1234
  slo $1234,x
  slo $1234,y
  slo ($34,x)
  slo ($34),y
  rla $34
  rla $34,x
  rla $1234
  rla $1234,x
  rla $1234,y
  rla ($34,x)
  rla ($34),y
  sre $34
  sre $34,x
  sre $1234
  sre $1234,x
  sre $1234,y
  sre ($34,x)
  sre ($34),y
  rra $34
  rra $34,x
  rra $1234
  rra $1234,x
  rra $1234,y
  rra ($34,x)
  rra ($34),y
  sax $34
  sax $34,y
  sax $1234
  sax ($34,x)
  lax $34
  lax $34,y
  lax $1234
  lax $1234,y
  lax ($34,x)
  lax ($34),y
  dcp $34
  dcp $34,x
  dcp $1234
  dcp $1234,x
  dcp $1234,y
  dcp ($34,x)
  dcp ($34),y
  isc $34
  isc $34,x
  isc $1234
  isc $1234,x
  isc $1234,y
  isc ($34,x)
  isc ($34),y
  anc #$12
  asr #$12
  arr #$12
  sbx #$12
zp = $fb
  lda zp
  sta zp,x
  lda (zp),y
  lda later
  jmp (vector)
- inx
  bne -
  beq +
  nop
+ rts
vector: !word start, later
  !byte 1,2,3,<start,>start
  !text "hello"
  !fill 3,$aa
later: !by $ff
  !wo $1234
