; Minimal highlights for tree-sitter-kotlin-ng (the crate ships none).
[
  "abstract" "annotation" "as" "by" "catch" "class" "companion" "const" "constructor"
  "data" "do" "else" "enum" "final" "finally" "for" "fun" "if" "import" "in" "init"
  "inline" "inner" "interface" "internal" "is" "lateinit" "object" "open" "operator"
  "out" "override" "package" "private" "protected" "public" "return" "sealed" "suspend"
  "super" "this" "throw" "try" "typealias" "val" "var" "vararg" "when" "while"
] @keyword

(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string
(number_literal) @number
(float_literal) @number
(user_type) @type
(annotation) @attribute
