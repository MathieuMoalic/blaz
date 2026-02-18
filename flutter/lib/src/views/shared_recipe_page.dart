import 'dart:ui' show FontFeature;
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../api.dart' as api;

/// Read-only recipe view shown when visiting a /share/<token> URL.
class SharedRecipePage extends StatefulWidget {
  final String token;
  const SharedRecipePage({super.key, required this.token});

  @override
  State<SharedRecipePage> createState() => _SharedRecipePageState();
}

class _SharedRecipePageState extends State<SharedRecipePage> {
  static const double kcalPerGProt = 4.27;
  static const double kcalPerGCarb = 3.87;
  static const double kcalPerGFat = 8.79;

  late Future<api.Recipe> _future;
  final Set<int> _checkedIngredients = {};
  final Set<int> _checkedSteps = {};
  double _scale = 1.0;

  @override
  void initState() {
    super.initState();
    _future = api.fetchSharedRecipe(widget.token);
  }

  void _toggleIngredient(int i) => setState(() {
        if (!_checkedIngredients.add(i)) _checkedIngredients.remove(i);
      });

  void _toggleStep(int i) => setState(() {
        if (!_checkedSteps.add(i)) _checkedSteps.remove(i);
      });

  double _calcCalories(api.RecipeMacros m) =>
      m.protein * kcalPerGProt + m.carbs * kcalPerGCarb + m.fat * kcalPerGFat;

  String _fmtTs(String s) {
    try {
      final dt = DateTime.parse(s.replaceFirst(' ', 'T'));
      return DateFormat.yMMMd().add_Hm().format(dt);
    } catch (_) {
      return s;
    }
  }

  void _openImageViewer({required String fullUrl, required String heroTag}) {
    Navigator.of(context, rootNavigator: true).push(
      PageRouteBuilder(
        opaque: false,
        barrierColor: Colors.black.withValues(alpha: 0.95),
        pageBuilder: (_, __, ___) => _ImageViewerPage(url: fullUrl, heroTag: heroTag),
        transitionsBuilder: (_, anim, __, child) =>
            FadeTransition(opacity: anim, child: child),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Shared Recipe')),
      body: FutureBuilder<api.Recipe>(
        future: _future,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snap.hasError) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Text(
                  '${snap.error}',
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.bodyLarge,
                ),
              ),
            );
          }
          final r = snap.data!;
          final small = api.mediaUrl(r.imagePathSmall);
          final full = api.mediaUrl(r.imagePathFull);
          final heroTag = 'shared-recipe-image';

          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Text(r.title, style: Theme.of(context).textTheme.headlineSmall),
              const SizedBox(height: 8),
              if (small != null) ...[
                Hero(
                  tag: heroTag,
                  child: Material(
                    borderRadius: BorderRadius.circular(10),
                    clipBehavior: Clip.antiAlias,
                    child: InkWell(
                      onTap: () => _openImageViewer(
                        fullUrl: full ?? small,
                        heroTag: heroTag,
                      ),
                      child: Ink.image(
                        image: NetworkImage(small),
                        fit: BoxFit.cover,
                        height: 250,
                        width: double.infinity,
                      ),
                    ),
                  ),
                ),
                const SizedBox(height: 12),
              ],

              // Ingredients + scale
              Card(
                margin: const EdgeInsets.only(top: 4, bottom: 12),
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('Ingredients', style: Theme.of(context).textTheme.titleMedium),
                      const SizedBox(height: 8),
                      Row(
                        children: [
                          Text('Scale', style: Theme.of(context).textTheme.bodyLarge),
                          const SizedBox(width: 10),
                          DropdownButton<double>(
                            value: _scale,
                            onChanged: (v) => setState(() => _scale = v ?? 1.0),
                            items: const [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0]
                                .map((v) => DropdownMenuItem(
                                      value: v,
                                      child: Text('${v}x'),
                                    ))
                                .toList(),
                          ),
                          const SizedBox(width: 8),
                          TextButton(
                            onPressed: () => setState(() => _scale = 1.0),
                            child: const Text('Reset'),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      if (r.ingredients.isEmpty)
                        const Text('—')
                      else
                        ...r.ingredients.asMap().entries.map((e) {
                          final idx = e.key;
                          final ing = e.value;
                          return _Bullet(
                            text: ing.toLine(factor: _scale),
                            checked: _checkedIngredients.contains(idx),
                            onTap: () => _toggleIngredient(idx),
                          );
                        }),
                    ],
                  ),
                ),
              ),

              // Instructions
              Card(
                margin: const EdgeInsets.only(bottom: 12),
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('Instructions', style: Theme.of(context).textTheme.titleMedium),
                      const SizedBox(height: 8),
                      if (r.instructions.isEmpty)
                        const Text('—')
                      else
                        for (int i = 0; i < r.instructions.length; i++)
                          _Numbered(
                            step: i + 1,
                            text: r.instructions[i],
                            checked: _checkedSteps.contains(i),
                            onTap: () => _toggleStep(i),
                          ),
                    ],
                  ),
                ),
              ),

              // Notes
              if (r.notes.isNotEmpty)
                Card(
                  margin: const EdgeInsets.only(bottom: 12),
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Notes', style: Theme.of(context).textTheme.titleMedium),
                        const SizedBox(height: 8),
                        Text(r.notes, style: Theme.of(context).textTheme.bodyLarge),
                      ],
                    ),
                  ),
                ),

              // Meta
              const SizedBox(height: 16),
              _MetaRow(label: 'Source', value: r.source.isEmpty ? '—' : r.source),
              _MetaRow(label: 'Yield', value: r.yieldText.isEmpty ? '—' : r.yieldText),
              _MetaRow(
                label: 'Created',
                value: r.createdAt.isEmpty ? '—' : _fmtTs(r.createdAt),
              ),

              // Macros
              if (r.macros != null) ...[
                const SizedBox(height: 18),
                _MacrosSection(
                  macros: r.macros!,
                  calcCalories: _calcCalories,
                ),
              ],
            ],
          );
        },
      ),
    );
  }
}

// ---- Small UI helpers -------------------------------------------------------

class _MetaRow extends StatelessWidget {
  final String label;
  final String value;
  const _MetaRow({required this.label, required this.value});
  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          SizedBox(
            width: 90,
            child: Text(label, style: Theme.of(context).textTheme.bodySmall),
          ),
          Expanded(
            child: Text(value, style: Theme.of(context).textTheme.bodyMedium),
          ),
        ],
      ),
    );
  }
}

class _Bullet extends StatelessWidget {
  final String text;
  final bool checked;
  final VoidCallback onTap;
  const _Bullet({required this.text, required this.checked, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final base = Theme.of(context).textTheme.bodyLarge;
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked ? (base.color ?? Colors.black).withValues(alpha: 0.55) : base.color,
      height: 1.3,
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('•  ', style: base),
            Expanded(
              child: AnimatedDefaultTextStyle(
                duration: const Duration(milliseconds: 120),
                style: style ?? const TextStyle(),
                child: Text(text),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Numbered extends StatelessWidget {
  final int step;
  final String text;
  final bool checked;
  final VoidCallback onTap;
  const _Numbered({
    required this.step,
    required this.text,
    required this.checked,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final base = Theme.of(context).textTheme.bodyLarge;
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked ? (base.color ?? Colors.black).withValues(alpha: 0.55) : base.color,
      height: 1.3,
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('$step. ', style: base),
            Expanded(
              child: AnimatedDefaultTextStyle(
                duration: const Duration(milliseconds: 120),
                style: style ?? const TextStyle(),
                child: Text(text),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _MacrosSection extends StatelessWidget {
  final api.RecipeMacros macros;
  final double Function(api.RecipeMacros) calcCalories;
  const _MacrosSection({required this.macros, required this.calcCalories});

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final m = macros;
    final kcal = calcCalories(m).clamp(0, double.infinity);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Nutrition (per recipe)', style: t.titleMedium),
        const SizedBox(height: 8),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Table(
              columnWidths: const {
                0: FlexColumnWidth(3),
                1: FixedColumnWidth(55),
                2: FixedColumnWidth(55),
                3: FixedColumnWidth(55),
                4: FixedColumnWidth(60),
              },
              defaultVerticalAlignment: TableCellVerticalAlignment.middle,
              children: [
                TableRow(children: [
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('', style: t.labelSmall),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('P (g)', style: t.labelSmall, textAlign: TextAlign.right),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('F (g)', style: t.labelSmall, textAlign: TextAlign.right),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('C (g)', style: t.labelSmall, textAlign: TextAlign.right),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('kcal', style: t.labelSmall, textAlign: TextAlign.right),
                  ),
                ]),
                TableRow(children: [
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 6),
                    child: Text('Total', style: t.titleSmall),
                  ),
                  ...[m.protein, m.fat, m.carbs].map((v) => Padding(
                        padding: const EdgeInsets.symmetric(vertical: 6),
                        child: Text(
                          v.round().toString(),
                          style: t.bodyMedium?.copyWith(
                            fontWeight: FontWeight.bold,
                            fontFeatures: [const FontFeature.tabularFigures()],
                          ),
                          textAlign: TextAlign.right,
                        ),
                      )),
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 6),
                    child: Text(
                      kcal.round().toString(),
                      style: t.bodyMedium?.copyWith(
                        fontWeight: FontWeight.bold,
                        fontFeatures: [const FontFeature.tabularFigures()],
                      ),
                      textAlign: TextAlign.right,
                    ),
                  ),
                ]),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _ImageViewerPage extends StatelessWidget {
  final String url;
  final String heroTag;
  const _ImageViewerPage({required this.url, required this.heroTag});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => Navigator.of(context).pop(),
      child: Scaffold(
        backgroundColor: Colors.transparent,
        body: Center(
          child: Hero(
            tag: heroTag,
            child: Image.network(url, fit: BoxFit.contain),
          ),
        ),
      ),
    );
  }
}
