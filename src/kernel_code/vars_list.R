# Run by Limoniter on a notebook's R kernel to list its variables, for the Variables panel. It prints one line of
# JSON between two markers, and leaves nothing behind in the session.
local({
  esc <- function(s) {
    s <- enc2utf8(as.character(s))
    s <- gsub("\\", "\\\\", s, fixed = TRUE)
    s <- gsub("\"", "\\\"", s, fixed = TRUE)
    s <- gsub("\n", "\\n", s, fixed = TRUE)
    s <- gsub("\r", "\\r", s, fixed = TRUE)
    s <- gsub("\t", "\\t", s, fixed = TRUE)
    s <- gsub("[[:cntrl:]]", " ", s)
    paste0("\"", s, "\"")
  }
  times <- intToUtf8(215)
  items <- character()
  for (n in ls(globalenv())) {
    v <- tryCatch(get(n, envir = globalenv()), error = function(e) NULL)
    if (is.null(v) || is.function(v) || is.environment(v)) next
    kind <- ""
    size <- ""
    d <- dim(v)
    if (is.data.frame(v)) {
      kind <- "frame"
      size <- paste(d, collapse = paste0(" ", times, " "))
    } else if (is.matrix(v) && length(d) == 2) {
      kind <- "array"
      size <- paste(d, collapse = paste0(" ", times, " "))
    } else if (is.list(v) && length(v) > 0 && !is.null(names(v)) && length(v) <= 5000 &&
               all(vapply(v, function(x) is.atomic(x) && is.null(dim(x)), TRUE)) && length(unique(lengths(v))) == 1 && lengths(v)[1] > 0) {
      kind <- "columns"
      size <- as.character(length(v))
    } else if (!is.null(d)) {
      size <- paste(d, collapse = paste0(" ", times, " "))
    } else {
      size <- as.character(length(v))
    }
    value <- ""
    if (is.atomic(v) && length(v) == 1 && !is.factor(v)) value <- substr(paste(format(v), collapse = " "), 1, 60)
    items <- c(items, paste0("{\"name\":", esc(n), ",\"type\":", esc(class(v)[1]), ",\"size\":", esc(size), ",\"kind\":", esc(kind), ",\"value\":", esc(value), "}"))
  }
  cat("@@LIMONITER@@[", paste(items, collapse = ","), "]@@LIMONITER@@", sep = "")
})
