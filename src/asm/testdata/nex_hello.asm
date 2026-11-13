  device zxspectrumnext
  org $8000
start:
  nextreg $07,3
  ld a,2
  out ($fe),a
loop:
  jr loop
  savenex open "nex_hello.nex",start,$fffe
  savenex core 0,0,0
  savenex auto
  savenex close
