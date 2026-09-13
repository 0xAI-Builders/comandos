"""Ordering shared by native tabs and the remote tab registry."""


def ordered_tab_keys(keys, favorites=()):
    favorites = set(favorites)
    return sorted(keys, key=lambda key: 0 if key == "local" else 1 if key in favorites else 2)
