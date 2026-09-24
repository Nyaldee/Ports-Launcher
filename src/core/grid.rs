
pub const CARD_WIDTH: f32 = 200.0;
pub const CARD_HEIGHT: f32 = 300.0;
pub const CARD_SPACING: f32 = 14.0;

pub fn compute_grid_columns(available_width: f32) -> usize {
    ((available_width / (CARD_WIDTH + CARD_SPACING)).floor() as usize).max(1)
}

pub fn chunk_into_rows<T: Clone>(items: &[T], columns: usize) -> Vec<Vec<T>> {
    items.chunks(columns.max(1)).map(<[T]>::to_vec).collect()
}

pub fn next_grid_position(current: (usize, usize), dx: i32, dy: i32, columns: usize, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let columns = columns.max(1);
    let flat = (current.0 * columns + current.1) as i32;
    let next_flat = (flat + dx + dy * columns as i32).clamp(0, len as i32 - 1) as usize;
    Some((next_flat / columns, next_flat % columns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colonnes_arrondies_vers_le_bas() {
        assert_eq!(compute_grid_columns(988.0), 4);
    }

    #[test]
    fn colonnes_jamais_zero() {
        assert_eq!(compute_grid_columns(0.0), 1);
        assert_eq!(compute_grid_columns(-50.0), 1);
    }

    #[test]
    fn chunk_groupe_par_lignes_avec_reste() {
        assert_eq!(chunk_into_rows(&[1, 2, 3, 4, 5], 2), vec![vec![1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn chunk_colonnes_zero_traite_comme_une_seule_colonne() {
        assert_eq!(chunk_into_rows(&[1, 2], 0), vec![vec![1], vec![2]]);
    }

    #[test]
    fn next_grid_position_reste_toujours_dans_les_bornes_reelles() {
        for len in 0..=37usize {
            for columns in 1..=8usize {
                for start_flat in 0..len.max(1) {
                    let start = (start_flat / columns, start_flat % columns);
                    for dx in [-5, -1, 0, 1, 5] {
                        for dy in [-5, -1, 0, 1, 5] {
                            match next_grid_position(start, dx, dy, columns, len) {
                                Some((row, col)) => assert!(row * columns + col < len, "len={len} columns={columns} start={start:?} dx={dx} dy={dy}"),
                                None => assert_eq!(len, 0),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn next_grid_position_carte_seule_sur_derniere_ligne_incomplete() {
        let (len, columns) = (7, 4);
        assert_eq!(next_grid_position((1, 2), 1, 0, columns, len), Some((1, 2)));
        assert_eq!(next_grid_position((1, 2), 5, 0, columns, len), Some((1, 2)));
        assert_eq!(next_grid_position((0, 0), 0, 1, columns, len), Some((1, 0)));
    }
}
