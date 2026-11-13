  device zxspectrumnext
  org $8000
  swapnib
  mirror a
  test $55
  bsla de,b
  bsra de,b
  bsrl de,b
  bsrf de,b
  brlc de,b
  mul d,e
  add hl,a
  add de,a
  add bc,a
  add hl,$1234
  add de,$1234
  add bc,$1234
  push $1234
  outinb
  nextreg $07,3
  nextreg $07,a
  pixeldn
  pixelad
  setae
  jp (c)
  ldix
  ldws
  lddx
  ldirx
  ldpirx
  lddrx
