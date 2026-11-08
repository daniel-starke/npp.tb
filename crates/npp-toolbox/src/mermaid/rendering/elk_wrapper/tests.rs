use super::*;

#[test]
fn breaks_cycles_in_declaration_order() {
	// A -> B -> C -> A: the last edge closes the cycle. C -> B closes one with B -> C.
	let ends = [(0, 1), (1, 2), (2, 0), (2, 1), (0, 0), (0, 9)];
	assert_eq!(edges_closing_cycles(&ends, 3), [false, false, true, true, false, false]);
	// The earlier edge wins against the later one in the other direction.
	assert_eq!(edges_closing_cycles(&[(1, 0), (0, 1)], 2), [false, true]);
}
