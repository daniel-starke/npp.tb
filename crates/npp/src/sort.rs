//! Stable sorting with a single compiled sort.
//!
//! `slice::sort_by` compiles the whole sort (about 6 KB) for every element type and comparison
//! closure. These functions sort the indices of the items with one non-generic stable sort and
//! then move the items into that order, so a call site adds only a few hundred bytes. The result
//! is that of `slice::sort_by`: stable, ordered by the same comparison.

use std::cmp::Ordering;

/// `items.sort_by(compare)`.
pub fn sort_by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
	if items.len() < 2 {
		return;
	}
	let mut order =
		sorted_indices(items.len(), &mut |first, second| compare(&items[first], &items[second]));
	apply_order(items, &mut order);
}

/// `items.sort_by_key(key)`: like the standard library, computes the key for every comparison.
pub fn sort_by_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
	sort_by(items, |first, second| key(first).cmp(&key(second)));
}

/// The indices `0..length` in the stable order of `compare`.
#[inline(never)]
fn sorted_indices(length: usize, compare: &mut dyn FnMut(usize, usize) -> Ordering) -> Vec<usize> {
	let mut order: Vec<usize> = (0..length).collect();
	order.sort_by(|&first, &second| compare(first, second));
	order
}

/// Moves `items[order[i]]` to position `i`, following each cycle of the permutation once.
/// `order` is consumed: every visited entry is set to its own position.
fn apply_order<T>(items: &mut [T], order: &mut [usize]) {
	for start in 0..order.len() {
		let mut current = start;
		loop {
			let next = order[current];
			order[current] = current;
			if next == start || next == current {
				break;
			}
			items.swap(current, next);
			current = next;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A small linear congruential generator, so the test needs no crate.
	struct Random(u64);

	impl Random {
		fn next(&mut self, bound: u64) -> u64 {
			self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
			(self.0 >> 33) % bound
		}
	}

	#[test]
	fn sorts_like_the_standard_library() {
		let mut random = Random(7);
		for _ in 0..2000 {
			let length = random.next(40) as usize;
			let key_range = 1 + random.next(12);
			// Pairs of key and original position show whether equal keys keep their order.
			let items: Vec<(u64, usize)> =
				(0..length).map(|position| (random.next(key_range), position)).collect();
			let mut expected = items.clone();
			expected.sort_by_key(|item| item.0);
			let mut sorted = items.clone();
			sort_by(&mut sorted, |first, second| first.0.cmp(&second.0));
			assert_eq!(sorted, expected);
			let mut by_key = items;
			sort_by_key(&mut by_key, |item| item.0);
			assert_eq!(by_key, expected);
		}
	}

	#[test]
	fn sorts_owned_values() {
		let mut words: Vec<String> = ["pear", "apple", "fig", "kiwi", "banana"]
			.iter()
			.map(|word| word.to_string())
			.collect();
		sort_by_key(&mut words, |word| word.len());
		assert_eq!(words, ["fig", "pear", "kiwi", "apple", "banana"]);
	}
}
