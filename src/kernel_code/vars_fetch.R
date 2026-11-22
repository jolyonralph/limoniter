# Run by Limoniter on a notebook's R kernel to get the first rows of one variable, for the data viewer.
# NAME and LIMIT are put in by Limoniter. It prints one line of JSON between two markers.
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
  v <- get(NAME, envir = globalenv())
  limit <- LIMIT
  if (is.matrix(v) && !is.data.frame(v)) {
    total <- nrow(v)
    df <- as.data.frame(utils::head(v, limit))
    if (is.null(colnames(v))) names(df) <- paste0("V", seq_len(ncol(df)))
  } else if (is.data.frame(v)) {
    total <- nrow(v)
    df <- utils::head(v, limit)
  } else {
    df <- as.data.frame(v, stringsAsFactors = FALSE)
    total <- nrow(df)
    df <- utils::head(df, limit)
  }
  kinds <- character()
  cols <- list()
  for (i in seq_along(df)) {
    x <- df[[i]]
    if (is.logical(x)) {
      kinds[i] <- "bool"
      cols[[i]] <- ifelse(is.na(x), "null", ifelse(x, "true", "false"))
    } else if (is.integer(x)) {
      kinds[i] <- "int"
      cols[[i]] <- ifelse(is.na(x), "null", as.character(x))
    } else if (is.numeric(x) && !inherits(x, c("Date", "POSIXct", "difftime"))) {
      kinds[i] <- "real"
      cols[[i]] <- ifelse(is.na(x) | is.nan(x), "null", ifelse(is.infinite(x), esc(x), as.character(x)))
    } else {
      kinds[i] <- "unknown"
      if (is.list(x)) x <- vapply(x, function(e) paste(format(e), collapse = ", "), "")
      x <- as.character(x)
      cols[[i]] <- ifelse(is.na(x), "null", esc(x))
    }
  }
  rows <- if (nrow(df) == 0 || length(cols) == 0) character() else paste0("[", do.call(paste, c(cols, sep = ",")), "]")
  cat("@@LIMONITER@@{\"columns\":[", paste(esc(names(df)), collapse = ","), "],\"kinds\":[", paste(esc(kinds), collapse = ","),
      "],\"rows\":[", paste(rows, collapse = ","), "],\"total\":", total, "}@@LIMONITER@@", sep = "")
})
