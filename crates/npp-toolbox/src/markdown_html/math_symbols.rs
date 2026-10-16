//! LaTeX math commands that stand for a symbol, accent or delimiter.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
	/// A variable such as a lower case Greek letter, italic by default.
	Identifier(&'static str),
	/// An upright identifier such as an upper case Greek letter.
	UprightIdentifier(&'static str),
	Operator(&'static str),
	/// `\sum`, `\int`: `true` if the scripts become limits above and below in display math.
	LargeOperator(&'static str, bool),
	/// `\sin`: a function name.
	Function(&'static str),
	/// `\lim`, `\max`: a function name with limits below in display math.
	LimitFunction(&'static str),
	/// Horizontal space of the given width.
	Space(&'static str),
}

pub fn symbol(name: &str) -> Option<Symbol> {
	use Symbol::{
		Function, Identifier, LargeOperator, LimitFunction, Operator, Space, UprightIdentifier,
	};
	Some(match name {
		"alpha" => Identifier("α"),
		"beta" => Identifier("β"),
		"gamma" => Identifier("γ"),
		"delta" => Identifier("δ"),
		"epsilon" => Identifier("ϵ"),
		"varepsilon" => Identifier("ε"),
		"zeta" => Identifier("ζ"),
		"eta" => Identifier("η"),
		"theta" => Identifier("θ"),
		"vartheta" => Identifier("ϑ"),
		"iota" => Identifier("ι"),
		"kappa" => Identifier("κ"),
		"varkappa" => Identifier("ϰ"),
		"lambda" => Identifier("λ"),
		"mu" => Identifier("μ"),
		"nu" => Identifier("ν"),
		"xi" => Identifier("ξ"),
		"omicron" => Identifier("ο"),
		"pi" => Identifier("π"),
		"varpi" => Identifier("ϖ"),
		"rho" => Identifier("ρ"),
		"varrho" => Identifier("ϱ"),
		"sigma" => Identifier("σ"),
		"varsigma" => Identifier("ς"),
		"tau" => Identifier("τ"),
		"upsilon" => Identifier("υ"),
		"phi" => Identifier("ϕ"),
		"varphi" => Identifier("φ"),
		"chi" => Identifier("χ"),
		"psi" => Identifier("ψ"),
		"omega" => Identifier("ω"),
		"Gamma" => UprightIdentifier("Γ"),
		"Delta" => UprightIdentifier("Δ"),
		"Theta" => UprightIdentifier("Θ"),
		"Lambda" => UprightIdentifier("Λ"),
		"Xi" => UprightIdentifier("Ξ"),
		"Pi" => UprightIdentifier("Π"),
		"Sigma" => UprightIdentifier("Σ"),
		"Upsilon" => UprightIdentifier("Υ"),
		"Phi" => UprightIdentifier("Φ"),
		"Psi" => UprightIdentifier("Ψ"),
		"Omega" => UprightIdentifier("Ω"),
		"infty" => UprightIdentifier("∞"),
		"partial" => UprightIdentifier("∂"),
		"nabla" => UprightIdentifier("∇"),
		"hbar" | "hslash" => Identifier("ℏ"),
		"ell" => Identifier("ℓ"),
		"wp" => Identifier("℘"),
		"Re" => UprightIdentifier("ℜ"),
		"Im" => UprightIdentifier("ℑ"),
		"aleph" => UprightIdentifier("ℵ"),
		"beth" => UprightIdentifier("ℶ"),
		"emptyset" => UprightIdentifier("∅"),
		"varnothing" => UprightIdentifier("⌀"),
		"imath" => Identifier("ı"),
		"jmath" => Identifier("ȷ"),
		"degree" => UprightIdentifier("°"),
		"top" => UprightIdentifier("⊤"),
		"bot" => UprightIdentifier("⊥"),
		"angle" => UprightIdentifier("∠"),
		"triangle" => UprightIdentifier("△"),
		"square" | "Box" => UprightIdentifier("□"),
		"blacksquare" => UprightIdentifier("■"),
		"diamond" => Operator("⋄"),
		"clubsuit" => UprightIdentifier("♣"),
		"diamondsuit" => UprightIdentifier("♢"),
		"heartsuit" => UprightIdentifier("♡"),
		"spadesuit" => UprightIdentifier("♠"),
		"checkmark" => UprightIdentifier("✓"),
		"dagger" => Operator("†"),
		"ddagger" => Operator("‡"),
		"prime" => Operator("′"),
		"forall" => Operator("∀"),
		"exists" => Operator("∃"),
		"nexists" => Operator("∄"),
		"neg" | "lnot" => Operator("¬"),
		"pm" => Operator("±"),
		"mp" => Operator("∓"),
		"times" => Operator("×"),
		"div" => Operator("÷"),
		"cdot" | "cdotp" => Operator("⋅"),
		"ast" => Operator("∗"),
		"star" => Operator("⋆"),
		"circ" => Operator("∘"),
		"bullet" => Operator("∙"),
		"oplus" => Operator("⊕"),
		"ominus" => Operator("⊖"),
		"otimes" => Operator("⊗"),
		"oslash" => Operator("⊘"),
		"odot" => Operator("⊙"),
		"cap" => Operator("∩"),
		"cup" => Operator("∪"),
		"sqcap" => Operator("⊓"),
		"sqcup" => Operator("⊔"),
		"uplus" => Operator("⊎"),
		"wedge" | "land" => Operator("∧"),
		"vee" | "lor" => Operator("∨"),
		"setminus" | "smallsetminus" => Operator("∖"),
		"wr" => Operator("≀"),
		"amalg" => Operator("⨿"),
		"lt" => Operator("<"),
		"gt" => Operator(">"),
		"leq" | "le" => Operator("≤"),
		"geq" | "ge" => Operator("≥"),
		"leqslant" => Operator("⩽"),
		"geqslant" => Operator("⩾"),
		"neq" | "ne" => Operator("≠"),
		"ll" => Operator("≪"),
		"gg" => Operator("≫"),
		"lll" => Operator("⋘"),
		"ggg" => Operator("⋙"),
		"approx" => Operator("≈"),
		"approxeq" => Operator("≊"),
		"equiv" => Operator("≡"),
		"sim" => Operator("∼"),
		"simeq" => Operator("≃"),
		"cong" => Operator("≅"),
		"asymp" => Operator("≍"),
		"propto" | "varpropto" => Operator("∝"),
		"doteq" => Operator("≐"),
		"triangleq" => Operator("≜"),
		"prec" => Operator("≺"),
		"succ" => Operator("≻"),
		"preceq" => Operator("⪯"),
		"succeq" => Operator("⪰"),
		"in" | "isin" => Operator("∈"),
		"notin" => Operator("∉"),
		"ni" | "owns" => Operator("∋"),
		"subset" => Operator("⊂"),
		"supset" => Operator("⊃"),
		"subseteq" => Operator("⊆"),
		"supseteq" => Operator("⊇"),
		"subsetneq" => Operator("⊊"),
		"supsetneq" => Operator("⊋"),
		"sqsubseteq" => Operator("⊑"),
		"sqsupseteq" => Operator("⊒"),
		"perp" => Operator("⊥"),
		"parallel" => Operator("∥"),
		"nparallel" => Operator("∦"),
		"mid" => Operator("∣"),
		"nmid" => Operator("∤"),
		"vdash" => Operator("⊢"),
		"dashv" => Operator("⊣"),
		"models" => Operator("⊨"),
		"therefore" => Operator("∴"),
		"because" => Operator("∵"),
		"colon" => Operator(":"),
		"to" | "rightarrow" => Operator("→"),
		"gets" | "leftarrow" => Operator("←"),
		"leftrightarrow" => Operator("↔"),
		"Rightarrow" => Operator("⇒"),
		"Leftarrow" => Operator("⇐"),
		"Leftrightarrow" => Operator("⇔"),
		"implies" | "Longrightarrow" => Operator("⟹"),
		"impliedby" | "Longleftarrow" => Operator("⟸"),
		"iff" | "Longleftrightarrow" => Operator("⟺"),
		"longrightarrow" => Operator("⟶"),
		"longleftarrow" => Operator("⟵"),
		"longleftrightarrow" => Operator("⟷"),
		"mapsto" => Operator("↦"),
		"longmapsto" => Operator("⟼"),
		"hookrightarrow" => Operator("↪"),
		"hookleftarrow" => Operator("↩"),
		"uparrow" => Operator("↑"),
		"downarrow" => Operator("↓"),
		"updownarrow" => Operator("↕"),
		"Uparrow" => Operator("⇑"),
		"Downarrow" => Operator("⇓"),
		"nearrow" => Operator("↗"),
		"searrow" => Operator("↘"),
		"swarrow" => Operator("↙"),
		"nwarrow" => Operator("↖"),
		"rightleftharpoons" => Operator("⇌"),
		"leftharpoonup" => Operator("↼"),
		"rightharpoonup" => Operator("⇀"),
		"ldots" | "dots" | "dotsc" | "dotso" | "mathellipsis" => Operator("…"),
		"cdots" | "dotsb" | "dotsm" | "dotsi" => Operator("⋯"),
		"vdots" => Operator("⋮"),
		"ddots" => Operator("⋱"),
		"langle" => Operator("⟨"),
		"rangle" => Operator("⟩"),
		"lfloor" => Operator("⌊"),
		"rfloor" => Operator("⌋"),
		"lceil" => Operator("⌈"),
		"rceil" => Operator("⌉"),
		"lbrace" => Operator("{"),
		"rbrace" => Operator("}"),
		"lbrack" => Operator("["),
		"rbrack" => Operator("]"),
		"vert" | "lvert" | "rvert" => Operator("|"),
		"Vert" | "lVert" | "rVert" => Operator("‖"),
		"backslash" => Operator("\\"),
		"sum" => LargeOperator("∑", true),
		"prod" => LargeOperator("∏", true),
		"coprod" => LargeOperator("∐", true),
		"bigcup" => LargeOperator("⋃", true),
		"bigcap" => LargeOperator("⋂", true),
		"bigsqcup" => LargeOperator("⨆", true),
		"biguplus" => LargeOperator("⨄", true),
		"bigoplus" => LargeOperator("⨁", true),
		"bigotimes" => LargeOperator("⨂", true),
		"bigodot" => LargeOperator("⨀", true),
		"bigvee" => LargeOperator("⋁", true),
		"bigwedge" => LargeOperator("⋀", true),
		"int" => LargeOperator("∫", false),
		"iint" => LargeOperator("∬", false),
		"iiint" => LargeOperator("∭", false),
		"oint" => LargeOperator("∮", false),
		"oiint" => LargeOperator("∯", false),
		"arccos" | "arcsin" | "arctan" | "arg" | "cos" | "cosh" | "cot" | "coth" | "csc"
		| "deg" | "dim" | "exp" | "hom" | "ker" | "lg" | "ln" | "log" | "sec" | "sin" | "sinh"
		| "tan" | "tanh" => Function(function_name(name)),
		"det" | "gcd" | "inf" | "lim" | "liminf" | "limsup" | "max" | "min" | "Pr" | "sup" => {
			LimitFunction(function_name(name))
		}
		"," | "thinspace" => Space("0.1667em"),
		":" | ">" | "medspace" => Space("0.2222em"),
		";" | "thickspace" => Space("0.2778em"),
		"!" | "negthinspace" => Space("-0.1667em"),
		"negmedspace" => Space("-0.2222em"),
		"negthickspace" => Space("-0.2778em"),
		" " | "space" | "nobreakspace" => Space("0.25em"),
		"enspace" => Space("0.5em"),
		"quad" => Space("1em"),
		"qquad" => Space("2em"),
		_ => return None,
	})
}

/// The name as `'static` text. The match arms above only pass known names.
fn function_name(name: &str) -> &'static str {
	const NAMES: [&str; 32] = [
		"arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "dim",
		"exp", "hom", "ker", "lg", "ln", "log", "sec", "sin", "sinh", "tan", "tanh", "det", "gcd",
		"inf", "lim", "liminf", "limsup", "max", "min", "Pr", "sup",
	];
	NAMES.iter().find(|known| **known == name).copied().unwrap_or("")
}

/// The `\not` form of a relation, e.g. `≠` for `=`.
pub fn negated(relation: &str) -> String {
	let negated = match relation {
		"=" => "≠",
		"<" => "≮",
		">" => "≯",
		"≤" => "≰",
		"≥" => "≱",
		"∈" => "∉",
		"∋" => "∌",
		"⊂" => "⊄",
		"⊃" => "⊅",
		"⊆" => "⊈",
		"⊇" => "⊉",
		"≡" => "≢",
		"∼" => "≁",
		"≈" => "≉",
		"≅" => "≇",
		"∣" => "∤",
		"∥" => "∦",
		_ => return format!("{relation}\u{338}"),
	};
	negated.to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccentPosition {
	Over,
	Under,
}

/// Accent commands: the accent character, its position and whether it stretches over the
/// whole base.
pub fn accent(name: &str) -> Option<(&'static str, AccentPosition, bool)> {
	use AccentPosition::{Over, Under};
	Some(match name {
		"hat" => ("^", Over, false),
		"widehat" => ("^", Over, true),
		"check" => ("ˇ", Over, false),
		"widecheck" => ("ˇ", Over, true),
		"tilde" => ("~", Over, false),
		"widetilde" => ("~", Over, true),
		"acute" => ("´", Over, false),
		"grave" => ("`", Over, false),
		"dot" => ("˙", Over, false),
		"ddot" => ("¨", Over, false),
		"dddot" => ("⃛", Over, false),
		"breve" => ("˘", Over, false),
		"bar" => ("¯", Over, false),
		"vec" => ("→", Over, false),
		"mathring" => ("˚", Over, false),
		"overline" => ("‾", Over, true),
		"overrightarrow" => ("→", Over, true),
		"overleftarrow" => ("←", Over, true),
		"overleftrightarrow" => ("↔", Over, true),
		"overbrace" => ("⏞", Over, true),
		"underline" => ("_", Under, true),
		"underbrace" => ("⏟", Under, true),
		"underrightarrow" => ("→", Under, true),
		"underleftarrow" => ("←", Under, true),
		_ => return None,
	})
}

/// The character of a delimiter after `\left`, `\right`, `\big` and so on. `Some("")` is the
/// invisible delimiter `.`.
pub fn delimiter_command(name: &str) -> Option<&'static str> {
	Some(match name {
		"langle" | "lang" => "⟨",
		"rangle" | "rang" => "⟩",
		"lfloor" => "⌊",
		"rfloor" => "⌋",
		"lceil" => "⌈",
		"rceil" => "⌉",
		"lbrace" => "{",
		"rbrace" => "}",
		"lbrack" => "[",
		"rbrack" => "]",
		"vert" | "lvert" | "rvert" => "|",
		"Vert" | "lVert" | "rVert" => "‖",
		"backslash" => "\\",
		"uparrow" => "↑",
		"downarrow" => "↓",
		"updownarrow" => "↕",
		"Uparrow" => "⇑",
		"Downarrow" => "⇓",
		"Updownarrow" => "⇕",
		_ => return None,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn function_names_are_known() {
		for name in ["sin", "lim", "Pr", "tanh"] {
			assert!(matches!(
				symbol(name),
				Some(Symbol::Function(found) | Symbol::LimitFunction(found)) if found == name
			));
		}
	}

	#[test]
	fn negates_relations() {
		assert_eq!(negated("="), "≠");
		assert_eq!(negated("⊢"), "⊢\u{338}");
	}
}
