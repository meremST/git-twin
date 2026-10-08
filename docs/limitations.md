# Limites connues de la version 0.2.0

- Les deux vues, menus, dialogues et interactions de base sont raccordes.
  Cela ne constitue pas encore une validation de toutes les lignes de
  `parity.md`. Les operations avancees (conflits, amend, rebase, outils externes,
  transport distant, signature) doivent recevoir leurs scenarios d'integration.
- Le rendu multipanneau est vise a 120 x 35. En dessous, jusqu'a 79 x 24,
  le mode compact montre un panneau a la fois. Cliquer les onglets ou utiliser
  Tab pour acceder aux autres panneaux ; les actions Worktree restent visibles.
  Les longs contenus (chemins, sujets) peuvent etre tronques, pas les commandes
  principales. Les tailles minimales des separateurs preservent les controles.
- L'editeur dispose d'annulation, selection, collage et edition externe.
  Le guide de largeur utilise les colonnes d'affichage Unicode, suit le
  defilement horizontal et ne remplace pas les caracteres du message.
- Les dialogues utilisent Tab, Shift-Tab, Entree et Echap. Les tres longs
  formulaires defilent avec la molette ou Alt-PageUp/PageDown/Home/End sans
  deplacer le focus. Les libelles se replient et les boutons restent accessibles.
- Ctrl-I/Tab et Ctrl-M/Entree ne sont pas distinguables sur tous les terminaux.
  Le protocole clavier enrichi est active si le terminal le declare.
  Les boutons et menus sont les alternatives ; Alt-Enter permet de committer.
- Un simple clic sur une icone de fichier indexe/desindexe. Un double-clic
  n'ajoute pas une seconde mutation. Les listes de git gui n'ont pas d'action
  specifique de double-clic ; ce n'est pas un raccourci pour indexer.
- La selection fine du diff (glisser ou Shift-Gauche/Droite) sert uniquement
  a copier. Shift-Haut/Bas selectionne des lignes Git pour indexer/annuler.
  Une tentative de mutation avec une selection fine est refusee explicitement.
  La selection native du
  terminal peut necessiter Shift lorsque la capture souris est active.
  `--no-mouse` laisse les evenements au terminal.
- Les actions interactives suspendent l'interface et rendent le terminal a Git
  ou a l'editeur. Leur statut est relu au retour. Les dialogues de signature,
  hooks et authentification n'ont pas encore ete verifies de bout en bout.
- La commande gitk permettant de generer des commits avec un shell arbitraire
  n'est pas implementee. Les outils configures explicitement et l'editeur Git
  s'executent seulement sur action utilisateur.
- L'application demande confirmation avant de quitter avec un message non vide
  lorsque `confirm_quit` est active. Un echec de sauvegarde empeche la fermeture
  normale ; le message reste en memoire et la sauvegarde peut etre reessayee.
  Les erreurs de lecture et de suppression des brouillons sont aussi signalees.
  L'option explicite de quitter sans sauvegarder abandonne les changements
  du message en memoire.
- Le plancher Git 2.25 est verifie au lancement, mais seule la version 2.43 a
  ete utilisee pour les essais actuels. macOS, gros depots et budgets de
  performances restent a valider.
- La distribution binaire cible Linux x86_64 GNU avec glibc >= 2.35 ;
  les executables ne sont pas statiques. Alpine/musl, ARM et Windows ne font
  pas partie de cette premiere distribution. Voir le
  [guide d'installation](build-install.md).
- Aucune capture des applications graphiques gitk/git gui n'a ete produite.
  L'exemple `preview` represente le rendu reel de Git Twin via ratatui TestBackend,
  et non une maquette statique.

Les commandes sont `gitt` (History) et `git tui` (Worktree, executable `git-tui`).
Le package et les chemins XDG restent nommes `git-twin` pour conserver les
preferences et brouillons existants.
