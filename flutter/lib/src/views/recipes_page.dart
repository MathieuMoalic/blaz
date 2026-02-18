import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import 'recipe_detail_page.dart';
import 'add_recipe_page.dart';

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  State<RecipesPage> createState() => RecipesPageState();
}

class RecipesPageState extends State<RecipesPage> {
  late Future<List<Recipe>> _future;
  final _filterCtrl = TextEditingController();
  final _searchFocus = FocusNode();
  String _query = '';
  bool _searchVisible = false;
  Timer? _debounce;

  // cache + soft loading flag
  List<Recipe> _cache = const <Recipe>[];
  bool _refreshing = false;

  @override
  void initState() {
    super.initState();
    _future = _loadInitial();
    _filterCtrl.addListener(_onFilterChanged);
  }

  Future<List<Recipe>> _loadInitial() async {
    final list = await fetchRecipes();
    _cache = list;
    return list;
  }

  @override
  void dispose() {
    _filterCtrl.removeListener(_onFilterChanged);
    _filterCtrl.dispose();
    _searchFocus.dispose();
    _debounce?.cancel();
    super.dispose();
  }

  void _toggleSearch() {
    setState(() {
      _searchVisible = !_searchVisible;
      if (_searchVisible) {
        _searchFocus.requestFocus();
      } else {
        _filterCtrl.clear();
        _searchFocus.unfocus();
      }
    });
  }

  void _onFilterChanged() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 200), () {
      if (!mounted) return;
      setState(() => _query = _filterCtrl.text.trim().toLowerCase());
    });
  }

  /// Refresh without blanking the grid.
  Future<void> refresh() async {
    if (_refreshing) return;
    setState(() => _refreshing = true);

    try {
      final list = await fetchRecipes();
      if (!mounted) return;

      setState(() {
        _cache = list;
        // Important: set to an already-completed future to avoid flicker.
        _future = Future.value(list);
      });
    } finally {
      if (mounted) {
        setState(() => _refreshing = false);
      }
    }
  }

  String _ymd(DateTime d) =>
      '${d.year.toString().padLeft(4, '0')}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

  Future<void> _assignRecipe(Recipe r) async {
    final now = DateTime.now();
    final picked = await showDatePicker(
      context: context,
      initialDate: now,
      firstDate: DateTime(now.year - 1),
      lastDate: DateTime(now.year + 2),
      locale: const Locale('en', 'GB'), // UK locale starts weeks on Monday
    );
    if (picked == null) return;

    final day = _ymd(picked);
    try {
      final entry = await assignRecipeToDay(day: day, recipeId: r.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Assigned "${r.title}" to ${entry.day}')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to assign: $e')));
    }
  }

  Future<void> _onAddRecipe() async {
    final created = await Navigator.of(
      context,
    ).push<bool>(MaterialPageRoute(builder: (_) => const AddRecipePage()));
    if (created == true) {
      await refresh();
    }
  }

  bool _matches(Recipe r, String q) {
    if (q.isEmpty) return true;
    return _fuzzyScore(r, q) > 0;
  }

  /// Fuzzy matching with scoring for better results ordering
  double _fuzzyScore(Recipe r, String q) {
    final needle = q.toLowerCase();
    final titleLower = r.title.toLowerCase();
    
    // Exact match (highest score)
    if (titleLower == needle) return 1000.0;
    
    // Title starts with query
    if (titleLower.startsWith(needle)) return 900.0;
    
    // Word boundary match (e.g., "chick" matches "Chicken Curry")
    final words = titleLower.split(RegExp(r'\s+'));
    for (final word in words) {
      if (word.startsWith(needle)) return 800.0;
    }
    
    // Contains exact substring
    if (titleLower.contains(needle)) return 700.0;
    
    // Fuzzy character sequence match
    double charScore = _fuzzyCharMatch(titleLower, needle);
    if (charScore > 0) return 500.0 + charScore;
    
    // Search in ingredients
    for (final ing in r.ingredients) {
      final ingLower = ing.name.toLowerCase();
      if (ingLower.contains(needle)) return 400.0;
      
      final ingCharScore = _fuzzyCharMatch(ingLower, needle);
      if (ingCharScore > 0) return 300.0 + ingCharScore;
    }
    
    // Search in instructions (lowest priority)
    for (final step in r.instructions) {
      if (step.toLowerCase().contains(needle)) return 200.0;
    }
    
    return 0.0;
  }

  /// Character-by-character fuzzy matching
  /// Returns score 0-100 based on how many characters match in sequence
  double _fuzzyCharMatch(String text, String pattern) {
    if (pattern.isEmpty) return 0;
    
    int patternIdx = 0;
    int lastMatchIdx = -1;
    int consecutiveMatches = 0;
    double score = 0;
    
    for (int i = 0; i < text.length && patternIdx < pattern.length; i++) {
      if (text[i] == pattern[patternIdx]) {
        // Bonus for consecutive matches
        if (i == lastMatchIdx + 1) {
          consecutiveMatches++;
          score += 10 + consecutiveMatches; // Increasing bonus
        } else {
          consecutiveMatches = 0;
          score += 5;
        }
        
        lastMatchIdx = i;
        patternIdx++;
      }
    }
    
    // All characters must match
    if (patternIdx != pattern.length) return 0;
    
    // Normalize score to 0-100
    return (score / pattern.length).clamp(0, 100);
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        if (_searchVisible)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
            child: TextField(
              controller: _filterCtrl,
              focusNode: _searchFocus,
              textInputAction: TextInputAction.search,
              decoration: InputDecoration(
                hintText: 'Search recipes...',
                prefixIcon: const Icon(Icons.search),
                isDense: true,
                suffixIcon: IconButton(
                  tooltip: 'Close',
                  icon: const Icon(Icons.close),
                  onPressed: _toggleSearch,
                ),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(12),
                ),
              ),
            ),
          ),

        Expanded(
          child: Stack(
            children: [
              RefreshIndicator(
                onRefresh: refresh,
                child: FutureBuilder<List<Recipe>>(
                  future: _future,
                  builder: (context, snap) {
                    // Use cache during loading to avoid flicker.
                    final items =
                        (snap.connectionState == ConnectionState.done &&
                            snap.data != null)
                        ? snap.data!
                        : _cache;

                    if (items.isEmpty) {
                      if (snap.hasError) {
                        return Center(child: Text('Error: ${snap.error}'));
                      }
                      if (snap.connectionState != ConnectionState.done) {
                        return const Center(child: CircularProgressIndicator());
                      }
                      return const _EmptyState();
                    }

                    // Filter and sort by relevance score
                    final filtered = items
                        .where((r) => _matches(r, _query))
                        .toList();
                    
                    // Sort by fuzzy match score (best matches first)
                    if (_query.isNotEmpty) {
                      filtered.sort((a, b) {
                        final scoreA = _fuzzyScore(a, _query);
                        final scoreB = _fuzzyScore(b, _query);
                        return scoreB.compareTo(scoreA); // Descending
                      });
                    }

                    if (filtered.isEmpty) {
                      return const _EmptyState();
                    }

                    return LayoutBuilder(
                      builder: (context, c) {
                        int cols = 2;
                        final w = c.maxWidth;
                        if (w >= 1200) {
                          cols = 5;
                        } else if (w >= 900) {
                          cols = 4;
                        } else if (w >= 600) {
                          cols = 3;
                        }

                        return GridView.builder(
                          padding: const EdgeInsets.fromLTRB(12, 4, 12, 12),
                          gridDelegate:
                              SliverGridDelegateWithFixedCrossAxisCount(
                                crossAxisCount: cols,
                                mainAxisSpacing: 12,
                                crossAxisSpacing: 12,
                                childAspectRatio: 3 / 4,
                              ),
                          itemCount: filtered.length,
                          itemBuilder: (_, i) {
                            final r = filtered[i];
                            final thumb = mediaUrl(r.imagePathSmall);

                            return _RecipeCard(
                              title: r.title,
                              imageUrl: thumb,
                              onOpen: () async {
                                await Navigator.of(context).push(
                                  MaterialPageRoute(
                                    builder: (_) =>
                                        RecipeDetailPage(recipeId: r.id),
                                  ),
                                );
                                await refresh();
                              },
                              onAssign: () => _assignRecipe(r),
                            );
                          },
                        );
                      },
                    );
                  },
                ),
              ),

              // Subtle top loading bar instead of full-screen spinner.
              if (_refreshing)
                const Positioned(
                  left: 0,
                  right: 0,
                  top: 0,
                  child: LinearProgressIndicator(minHeight: 2),
                ),

              // FABs: search (above) + add recipe (below)
              Positioned(
                right: 16,
                bottom: 88,
                child: FloatingActionButton(
                  heroTag: 'search',
                  onPressed: _toggleSearch,
                  tooltip: 'Search',
                  child: Icon(
                    _searchVisible ? Icons.search_off : Icons.search,
                  ),
                ),
              ),
              Positioned(
                right: 16,
                bottom: 16,
                child: FloatingActionButton(
                  heroTag: 'add',
                  onPressed: _onAddRecipe,
                  tooltip: 'Add recipe',
                  child: const Icon(Icons.add),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _RecipeCard extends StatelessWidget {
  final String title;
  final String? imageUrl;
  final VoidCallback? onOpen;
  final VoidCallback? onAssign;

  const _RecipeCard({
    required this.title,
    required this.imageUrl,
    this.onOpen,
    this.onAssign,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return InkWell(
      onTap: onOpen,
      borderRadius: BorderRadius.circular(16),
      child: Card(
        clipBehavior: Clip.antiAlias,
        elevation: 2,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Image with a tucked-away action in the corner
            Expanded(
              child: Stack(
                fit: StackFit.expand,
                children: [
                  imageUrl == null
                      ? _placeholder()
                      : Image.network(
                          imageUrl!,
                          fit: BoxFit.cover,
                          frameBuilder: (context, child, frame, wasSync) {
                            if (wasSync || frame != null) return child;
                            return const Center(
                              child: CircularProgressIndicator(),
                            );
                          },
                          errorBuilder: (_, __, ___) => _placeholder(),
                        ),
                  Positioned(
                    top: 8,
                    right: 8,
                    child: Material(
                      color: Colors.black54,
                      shape: const CircleBorder(),
                      child: IconButton(
                        tooltip: 'Assign to day',
                        icon: const Icon(
                          Icons.event,
                          size: 20,
                          color: Colors.white,
                        ),
                        onPressed: onAssign,
                        splashRadius: 22,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(12),
              child: Text(
                title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: theme.textTheme.titleMedium,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _placeholder() {
    return Container(
      alignment: Alignment.center,
      child: const Icon(Icons.restaurant_menu, size: 48),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    return ListView(
      physics: const AlwaysScrollableScrollPhysics(),
      children: const [
        SizedBox(height: 120),
        Icon(Icons.no_food, size: 48),
        SizedBox(height: 12),
        Center(
          child: Text(
            'No recipes match your filter.',
            textAlign: TextAlign.center,
          ),
        ),
      ],
    );
  }
}
