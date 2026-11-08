//! Mermaid's theme variables, computed by running the compiled theme code of `theme_table.rs`
//! with JavaScript's semantics.

use std::collections::HashMap;

use super::color::{self, Adjustment, js_number};
use super::theme_table::{self as code, FUNCTIONS, NUMBERS, STRINGS, THEMES};
use super::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calculate {
	/// Overrides, `updateColors`, overrides again.
	Standard,
	/// First resets all `'calculated'` placeholders to undefined.
	ClearsCalculated,
	/// Turns `useGradient` off when `nodeBorder` but not `useGradient` is overridden.
	NodeBorderDisablesGradient,
}

pub struct ThemeCode {
	pub constructor: &'static [u8],
	pub update_colors: &'static [u8],
	pub calculate: Calculate,
	pub local_count: usize,
}

/// Bounds the work of a theme, so that no override can make it loop.
const MAX_STEPS: usize = 1_000_000;
const MAX_STACK: usize = 256;

/// The variables of a theme; a missing key is JavaScript's `undefined`.
#[derive(Clone, Debug, Default)]
pub struct ThemeVariables {
	values: HashMap<String, Value>,
}

impl ThemeVariables {
	/// The theme `name` (Mermaid's theme names; unknown names are `default`) with the
	/// `themeVariables` overrides, like `getThemeVariables(overrides)`.
	pub fn new(name: &str, overrides: Option<&Value>) -> ThemeVariables {
		let code = THEMES
			.iter()
			.find(|(theme, _)| *theme == name)
			.or_else(|| THEMES.iter().find(|(theme, _)| *theme == "default"))
			.map(|(_, code)| code);
		let mut variables = ThemeVariables::default();
		let Some(code) = code else { return variables };
		let mut machine = Machine { code, variables: &mut variables, steps: 0 };
		machine.run(code.constructor);
		machine.calculate(overrides);
		variables
	}

	/// Whether Mermaid knows the theme `name`; `null` disables the themes and is valid too.
	pub fn exists(name: &str) -> bool {
		name == "null" || THEMES.iter().any(|(theme, _)| *theme == name)
	}

	pub fn get(&self, key: &str) -> Option<&Value> {
		self.values.get(key)
	}

	/// The variable as CSS text (strings, and numbers like JavaScript prints them).
	pub fn text(&self, key: &str) -> Option<String> {
		match self.values.get(key)? {
			Value::String(text) => Some(text.clone()),
			Value::Number(number) => Some(js_number(*number)),
			Value::Bool(value) => Some(value.to_string()),
			_ => None,
		}
	}

	/// [`Self::text`] or an empty string.
	pub fn css(&self, key: &str) -> String {
		self.text(key).unwrap_or_default()
	}

	fn set(&mut self, key: &str, value: Option<Value>) {
		match value {
			Some(value) => {
				if let Some(existing) = self.values.get_mut(key) {
					*existing = value;
				} else {
					self.values.insert(key.to_owned(), value);
				}
			}
			None => {
				self.values.remove(key);
			}
		}
	}
}

/// JavaScript's truthiness.
fn is_truthy(value: &Value) -> bool {
	match value {
		Value::Null => false,
		Value::Bool(value) => *value,
		Value::Number(number) => *number != 0.0 && !number.is_nan(),
		Value::String(text) => !text.is_empty(),
		Value::Array(_) | Value::Object(_) => true,
	}
}

fn is_truthy_option(value: &Option<Value>) -> bool {
	value.as_ref().is_some_and(is_truthy)
}

/// JavaScript's `String(value)`.
fn to_js_string(value: &Option<Value>) -> String {
	match value {
		None => "undefined".to_owned(),
		Some(Value::Null) => "null".to_owned(),
		Some(Value::Bool(value)) => value.to_string(),
		Some(Value::Number(number)) => js_number(*number),
		Some(Value::String(text)) => text.clone(),
		Some(Value::Array(items)) => {
			items.iter().map(|item| to_js_string(&Some(item.clone()))).collect::<Vec<_>>().join(",")
		}
		Some(Value::Object(_)) => "[object Object]".to_owned(),
	}
}

/// JavaScript's `Number(value)` for the operands the themes use.
fn to_js_number(value: &Option<Value>) -> f64 {
	match value {
		None => f64::NAN,
		Some(Value::Null) => 0.0,
		Some(Value::Bool(value)) => f64::from(u8::from(*value)),
		Some(Value::Number(number)) => *number,
		Some(Value::String(text)) => match text.trim() {
			"" => 0.0,
			trimmed => trimmed.parse().unwrap_or(f64::NAN),
		},
		Some(Value::Array(_) | Value::Object(_)) => f64::NAN,
	}
}

fn strict_equal(first: &Option<Value>, second: &Option<Value>) -> bool {
	match (first, second) {
		(None, None) => true,
		(Some(Value::Number(a)), Some(Value::Number(b))) => a == b,
		(Some(Value::String(a)), Some(Value::String(b))) => a == b,
		(Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
		(Some(Value::Null), Some(Value::Null)) => true,
		// Distinct objects are never identical; the themes only compare scalars.
		_ => false,
	}
}

struct Machine<'a> {
	code: &'a ThemeCode,
	variables: &'a mut ThemeVariables,
	steps: usize,
}

impl Machine<'_> {
	fn calculate(&mut self, overrides: Option<&Value>) {
		if self.code.calculate == Calculate::ClearsCalculated {
			self.variables
				.values
				.retain(|_, value| !matches!(value, Value::String(text) if text == "calculated"));
		}
		let Some(Value::Object(overrides)) = overrides else {
			self.run(self.code.update_colors);
			return;
		};
		for (key, value) in overrides {
			self.variables.set(key, Some(value.clone()));
		}
		self.run(self.code.update_colors);
		for (key, value) in overrides {
			// `applyOverride`: object variables keep the generated keys under the user's keys.
			let merged = match (self.variables.get(key), value) {
				(Some(Value::Object(generated)), Value::Object(user)) => {
					let mut merged = generated.clone();
					for (name, item) in user {
						merged.retain(|(known, _)| known != name);
						merged.push((name.clone(), item.clone()));
					}
					Value::Object(merged)
				}
				_ => value.clone(),
			};
			self.variables.set(key, Some(merged));
		}
		let overrides_key = |key: &str| overrides.iter().any(|(name, _)| name == key);
		if self.code.calculate == Calculate::NodeBorderDisablesGradient
			&& overrides_key("nodeBorder")
			&& !overrides_key("useGradient")
		{
			self.variables.set("useGradient", Some(Value::Bool(false)));
		}
	}

	/// Runs `program`. Malformed code or exhausted limits end the program early.
	fn run(&mut self, program: &[u8]) {
		let mut stack: Vec<Option<Value>> = Vec::with_capacity(16);
		let mut locals: Vec<Option<Value>> = vec![None; self.code.local_count];
		let mut position = 0;
		while position < program.len() {
			self.steps += 1;
			if self.steps > MAX_STEPS || stack.len() > MAX_STACK {
				return;
			}
			let opcode = program[position];
			let operand = match program.get(position + 1..position + 3) {
				Some(&[low, high]) => usize::from(u16::from_le_bytes([low, high])),
				_ => 0,
			};
			position += 1;
			let has_operand = !matches!(
				opcode,
				code::UNDEFINED
					| code::NULL | code::TRUE
					| code::FALSE | code::THIS_GET_DYNAMIC
					| code::THIS_SET_DYNAMIC
					| code::INDEX | code::ADD
					| code::SUBTRACT
					| code::MULTIPLY
					| code::DIVIDE | code::REMAINDER
					| code::NEGATE | code::NOT
					| code::STRICT_EQUAL
					| code::LESS | code::UPDATE_COLORS
					| code::POP
			);
			if has_operand {
				position += 2;
			}
			let string = |index: usize| STRINGS.get(index).copied().unwrap_or("");
			match opcode {
				code::UNDEFINED => stack.push(None),
				code::NULL => stack.push(Some(Value::Null)),
				code::TRUE => stack.push(Some(Value::Bool(true))),
				code::FALSE => stack.push(Some(Value::Bool(false))),
				code::STRING => stack.push(Some(Value::String(string(operand).to_owned()))),
				code::NUMBER => {
					stack.push(Some(Value::Number(
						NUMBERS.get(operand).copied().unwrap_or(f64::NAN),
					)));
				}
				code::THIS_GET => stack.push(self.variables.get(string(operand)).cloned()),
				code::THIS_GET_DYNAMIC => {
					let key = to_js_string(&stack.pop().flatten());
					stack.push(self.variables.get(&key).cloned());
				}
				code::THIS_SET => {
					let value = stack.pop().flatten();
					self.variables.set(string(operand), value);
				}
				code::THIS_SET_DYNAMIC => {
					let value = stack.pop().flatten();
					let key = to_js_string(&stack.pop().flatten());
					self.variables.set(&key, value);
				}
				code::PROPERTY => {
					let object = stack.pop().flatten();
					let name = string(operand);
					stack.push(property(object.as_ref(), name));
				}
				code::INDEX => {
					let index = stack.pop().flatten();
					let object = stack.pop().flatten();
					let value = match (&object, &index) {
						(Some(Value::Array(items)), Some(Value::Number(number))) => {
							let is_index = *number >= 0.0 && number.fract() == 0.0;
							is_index.then(|| items.get(*number as usize).cloned()).flatten()
						}
						(Some(_), Some(key)) => {
							property(object.as_ref(), &to_js_string(&Some(key.clone())))
						}
						_ => None,
					};
					stack.push(value);
				}
				code::LOCAL_GET => stack.push(locals.get(operand).cloned().flatten()),
				code::LOCAL_SET => {
					let value = stack.pop().flatten();
					if let Some(local) = locals.get_mut(operand) {
						*local = value;
					}
				}
				code::ADD => {
					let second = stack.pop().flatten();
					let first = stack.pop().flatten();
					let is_text = |value: &Option<Value>| {
						matches!(value, Some(Value::String(_) | Value::Array(_) | Value::Object(_)))
					};
					let sum = match is_text(&first) || is_text(&second) {
						true => Value::String(to_js_string(&first) + &to_js_string(&second)),
						false => Value::Number(to_js_number(&first) + to_js_number(&second)),
					};
					stack.push(Some(sum));
				}
				code::SUBTRACT | code::MULTIPLY | code::DIVIDE | code::REMAINDER => {
					let second = to_js_number(&stack.pop().flatten());
					let first = to_js_number(&stack.pop().flatten());
					let result = match opcode {
						code::SUBTRACT => first - second,
						code::MULTIPLY => first * second,
						code::DIVIDE => first / second,
						_ => first % second,
					};
					stack.push(Some(Value::Number(result)));
				}
				code::NEGATE => {
					let value = to_js_number(&stack.pop().flatten());
					stack.push(Some(Value::Number(-value)));
				}
				code::NOT => {
					let value = stack.pop().flatten();
					stack.push(Some(Value::Bool(!is_truthy_option(&value))));
				}
				code::STRICT_EQUAL => {
					let second = stack.pop().flatten();
					let first = stack.pop().flatten();
					stack.push(Some(Value::Bool(strict_equal(&first, &second))));
				}
				code::LESS => {
					let second = stack.pop().flatten();
					let first = stack.pop().flatten();
					let is_less = match (&first, &second) {
						(Some(Value::String(a)), Some(Value::String(b))) => a < b,
						_ => to_js_number(&first) < to_js_number(&second),
					};
					stack.push(Some(Value::Bool(is_less)));
				}
				code::JUMP => position = operand,
				code::JUMP_IF_FALSE => {
					if !is_truthy_option(&stack.pop().flatten()) {
						position = operand;
					}
				}
				code::OR | code::AND | code::NULLISH => {
					let top = stack.last().cloned().flatten();
					let keeps = match opcode {
						code::OR => is_truthy_option(&top),
						code::AND => !is_truthy_option(&top),
						_ => !matches!(top, None | Some(Value::Null)),
					};
					if keeps {
						position = operand;
					} else {
						stack.pop();
					}
				}
				code::CALL => {
					let (function, count) = (operand & 0xff, operand >> 8);
					let start = stack.len().saturating_sub(count);
					let arguments: Vec<Option<Value>> = stack.drain(start..).collect();
					stack.push(call(FUNCTIONS.get(function).copied().unwrap_or(""), &arguments));
				}
				code::OBJECT => {
					let start = stack.len().saturating_sub(operand * 2);
					let items: Vec<Option<Value>> = stack.drain(start..).collect();
					let entries = items
						.chunks(2)
						.filter_map(|pair| match pair {
							// JSON drops undefined object values; the themes read them back
							// as undefined either way.
							[key, Some(value)] => Some((to_js_string(key), value.clone())),
							_ => None,
						})
						.collect();
					stack.push(Some(Value::Object(entries)));
				}
				code::ARRAY => {
					let start = stack.len().saturating_sub(operand);
					let items =
						stack.drain(start..).map(|item| item.unwrap_or(Value::Null)).collect();
					stack.push(Some(Value::Array(items)));
				}
				code::UPDATE_COLORS => self.run(self.code.update_colors),
				code::POP => {
					stack.pop();
				}
				_ => return,
			}
		}
	}
}

fn property(object: Option<&Value>, name: &str) -> Option<Value> {
	match (object?, name) {
		(Value::Array(items), "length") => Some(Value::Number(items.len() as f64)),
		(Value::String(text), "length") => Some(Value::Number(text.encode_utf16().count() as f64)),
		(Value::Object(_), _) => object?.get(name).cloned(),
		_ => None,
	}
}

/// The `khroma` and helper functions of the themes. `None` (undefined) where `khroma` throws.
fn call(function: &str, arguments: &[Option<Value>]) -> Option<Value> {
	let text = |index: usize| match arguments.get(index) {
		Some(Some(Value::String(text))) => Some(text.as_str()),
		_ => None,
	};
	let number = |index: usize| to_js_number(arguments.get(index).unwrap_or(&None));
	let result = match function {
		"adjust" => {
			let channels = arguments.get(1).cloned().flatten();
			let channel = |name: &str| {
				channels
					.as_ref()
					.and_then(|channels| channels.get(name))
					.map_or(0.0, |value| to_js_number(&Some(value.clone())))
			};
			color::adjust(
				text(0)?,
				Adjustment {
					red: channel("r"),
					green: channel("g"),
					blue: channel("b"),
					hue: channel("h"),
					saturation: channel("s"),
					lightness: channel("l"),
					alpha: channel("a"),
				},
			)?
		}
		"darken" => color::darken(text(0)?, number(1))?,
		"lighten" => color::lighten(text(0)?, number(1))?,
		"invert" => {
			let weight = match arguments.get(1) {
				Some(Some(_)) => number(1),
				_ => 100.0,
			};
			color::invert(text(0)?, weight)?
		}
		"rgba" => match text(0) {
			Some(color) => color::with_alpha(color, number(1))?,
			None => {
				let blue = if matches!(arguments.get(2), Some(Some(_))) { number(2) } else { 0.0 };
				let alpha = if matches!(arguments.get(3), Some(Some(_))) { number(3) } else { 1.0 };
				color::rgba_of_numbers(number(0), number(1), blue, alpha)
			}
		},
		"isDark" => return Some(Value::Bool(color::is_dark(text(0)?)?)),
		"isLight" => return Some(Value::Bool(!color::is_dark(text(0)?)?)),
		"mkBorder" => {
			let is_dark_mode = is_truthy_option(arguments.get(1).unwrap_or(&None));
			color::make_border(text(0)?, is_dark_mode)?
		}
		"transparentize" => color::transparentize(text(0)?, number(1))?,
		_ => return None,
	};
	Some(Value::String(result))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::value::parse_json;

	/// Values of Mermaid 12.2.0's `getThemeVariables` under Node.js.
	#[test]
	fn computes_mermaid_theme_variables() {
		let theme = ThemeVariables::new("default", None);
		assert_eq!(theme.css("tertiaryColor"), "hsl(80, 100%, 96.2745098039%)");
		assert_eq!(theme.css("tertiaryTextColor"), "rgb(6.3333333334, 0, 19.0000000001)");
		assert_eq!(theme.css("primaryTextColor"), "#131300");
		assert_eq!(theme.css("cScale0"), "hsl(240, 100%, 76.2745098039%)");
		assert_eq!(theme.css("git0"), "hsl(240, 100%, 46.2745098039%)");
		assert_eq!(theme.css("quadrantPointFill"), "hsl(240, 100%, NaN%)");
		assert_eq!(theme.css("fontSize"), "16px");
		assert_eq!(theme.get("radius"), Some(&Value::Number(5.0)));
		let dark = ThemeVariables::new("dark", None);
		assert_eq!(dark.css("cScale0"), "#1f2020");
		assert_eq!(dark.css("git0"), "hsl(180, 1.5873015873%, 48.3529411765%)");
		assert_eq!(ThemeVariables::new("neutral", None).css("cScale0"), "#555");
		assert_eq!(ThemeVariables::new("redux-color", None).css("cScale0"), "#f4a8ff");
		assert_eq!(ThemeVariables::new("neo", None).css("fontSize"), "14px");
	}

	#[test]
	fn applies_overrides_like_mermaid() {
		let overrides =
			parse_json(r##"{"primaryColor": "#ff0000", "nodeBorder": "blue"}"##).unwrap();
		let theme = ThemeVariables::new("default", Some(&overrides));
		assert_eq!(theme.css("secondaryColor"), "#ffffde");
		assert_eq!(theme.css("cScale0"), "hsl(240, 100%, 76.2745098039%)");
		assert_eq!(theme.css("nodeBorder"), "blue");
		assert_eq!(theme.css("mainBkg"), "#ECECFF");
		let base = ThemeVariables::new("base", Some(&overrides));
		assert_eq!(base.get("useGradient"), Some(&Value::Bool(false)));
		let nested = parse_json(r##"{"xyChart": {"titleColor": "red"}}"##).unwrap();
		let theme = ThemeVariables::new("default", Some(&nested));
		let xy_chart = theme.get("xyChart").unwrap();
		assert_eq!(xy_chart.get("titleColor").and_then(Value::as_str), Some("red"));
		assert!(xy_chart.get("plotColorPalette").is_some());
	}

	#[test]
	fn survives_hostile_overrides() {
		for overrides in [
			r#"{"primaryColor": 5}"#,
			r#"{"THEME_COLOR_LIMIT": 1e9}"#,
			r#"{"darkMode": "yes", "background": []}"#,
		] {
			let overrides = parse_json(overrides).unwrap();
			for (name, _) in THEMES {
				let _ = ThemeVariables::new(name, Some(&overrides));
			}
		}
	}
}
