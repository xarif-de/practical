app-name = PractiCal
nav-calendar = Calendrier
nav-contacts = Contacts
nav-tasks = Tâches
nav-history = Modifications
nav-settings = Réglages
source-code = Code source (AGPL-3.0)
logout = Se déconnecter
scheme-toggle = Changer de thème
search-placeholder = Rechercher : événements, contacts, tâches, date, …
filter = Filtrer
filter-calendars = Calendriers
filter-addressbooks = Carnets d'adresses
filter-all = Tous
filter-none = Aucun
login-title = Connexion
login-with = Se connecter avec { $name }
login-redirecting = Redirection vers la connexion …
login-username = Nom d'utilisateur
login-password = Mot de passe
login-submit = Se connecter
login-failed = Échec de la connexion.
login-too-many = Trop de tentatives échouées – veuillez patienter une minute puis réessayer.
login-hint = Tout compte RustiCal peut se connecter. Un nouveau compte est créé lors de la première connexion sur { $dav }.
login-hint-signup = Tout compte de l’authentification unique peut se connecter. Un nouveau compte est créé ici lors de la première connexion.
login-no-account = Il n’existe pas encore de compte pour cette connexion.
login-create-account = Créer un compte sur RustiCal
login-id-rejected = Le nom d’utilisateur de l’authentification unique n’est pas autorisé comme nom de compte.
tab-account = Compte et apps
tab-groups = Groupes
tab-collections = Calendriers et carnets d'adresses
tab-tokens = Jetons d'app
tab-trash = Corbeille
tab-categories = Catégories
cat-hint = Une catégorie peut préremplir des réglages d'un événement (modèle, modifiable dans l'événement) ou les imposer (verrou, non modifiable dans l'événement). Avec plusieurs catégories, celle placée le plus haut dans l'événement l'emporte. Les verrous ne s'appliquent que dans cette interface ; les autres apps de calendrier peuvent tout modifier.
cat-rule-new = Créer une règle
cat-count = { $n ->
    [one] { $n } événement
   *[other] { $n } événements
}
cat-none = Aucune catégorie pour l'instant.
cat-new-placeholder = Nouvelle catégorie avec règle …
tab-general = Général
pref-sec-alarms = Rappels
profile-user = Utilisateur
profile-hint = Les apps de calendrier (DAVx⁵, Apple, Thunderbird) se connectent avec un jeton d'app, jamais avec le mot de passe SSO.
groups-intro = Un groupe appartient à ses membres : chaque membre peut ajouter ou retirer des membres, créer les calendriers et carnets d'adresses du groupe et supprimer le groupe.
group-name-placeholder = Nouveau groupe, p. ex. Famille
group-create = Créer le groupe
group-leave = Quitter
group-delete = Supprimer le groupe …
group-delete-explain = Supprime le groupe « { $name } » avec tous ses calendriers et carnets d'adresses.
group-purge-explain = Supprime définitivement le groupe « { $name } » avec tous ses calendriers et carnets d'adresses. Cette action est irréversible.
confirm-type = Saisir pour confirmer :
group-delete-confirm = Mettre à la corbeille
groups-none = Vous ne faites partie d'aucun groupe.
member-remove = retirer
member-pick = Choisir un membre …
member-add = Ajouter
n-calendars = { $n ->
    [one] 1 calendrier
   *[other] { $n } calendriers
}
n-addressbooks = { $n ->
    [one] 1 carnet d'adresses
   *[other] { $n } carnets d'adresses
}
trash-retention = { $days ->
    [one] Tout ce qui est dans la corbeille reste restaurable pendant { $days } jour.
   *[other] Tout ce qui est dans la corbeille reste restaurable pendant { $days } jours.
}
calendars = Calendriers
addressbooks = Carnets d'adresses
col-first-calendar = Mon agenda
col-first-addressbook = Mes contacts
col-name = Nom
col-owner = Appartient à
col-components = Contenu
col-color = Couleur
col-description = Description
col-created = Créé
group = Groupe
subscription = Abonnement
timezone = Fuseau horaire
save = Enregistrer
create = Créer
delete = Supprimer
restore = Restaurer
purge = Supprimer définitivement
purge-confirm = Supprimer définitivement
expires = expire le
deleted-by = supprimé par { $name }
calendar-create = Créer un calendrier …
calendar-import = Importer un calendrier …
set-import-calendar = Importer un calendrier
imp-choose = Depuis un fichier .ics …
imp-hint = La source devient un nouvel agenda. Nom, description, couleur et fuseau horaire sont repris lorsqu’ils y figurent — vous pouvez les changer avant l’import.
imp-comp-hint = Ce que contient le fichier reste activé.
imp-go = Importer
addressbook-create = Créer un carnet d'adresses …
addressbook-import = Importer un carnet d'adresses …
set-import-addressbook = Importer un carnet d'adresses
imp-ab-hint = La source devient un nouveau carnet d’adresses. Les fichiers vCard ne portent pas de nom : le nom du fichier est proposé — vous pouvez le changer avant l’import.
tokens-intro = Un jeton d'app est un mot de passe réservé à une seule app ou un seul appareil. Il n'est affiché qu'une seule fois.
token-name-placeholder = Appareil ou app, p. ex. téléphone DAVx⁵
token-create = Générer un jeton
token-new-hint = Nouveau jeton – copiez-le maintenant, il ne sera plus affiché :
copy = Copier
trash-empty = Vide.
pref-language = Langue
pref-scheme = Thème de couleurs
scheme-auto = Système
scheme-light = Clair
scheme-dark = Sombre
pref-vacation-days = Jours de congé par an
pref-snaps = Points d'accroche de la réglette
pref-snaps-hint = La réglette d'un événement s'accroche à ces points. Cliquez sur un point pour le modifier ou le supprimer. Le ＋ entre deux points ajoute la valeur ronde intermédiaire.
pref-snaps-reset = Rétablir les valeurs par défaut
confirm-remove-member = Retirer { $name } du groupe ?
confirm-leave = Quitter le groupe « { $name } » ?
confirm-trash-collection = Mettre « { $name } » à la corbeille ?
confirm-purge-collection = Supprimer « { $name } » définitivement ? Cette action est irréversible.
confirm-delete-token = Supprimer le jeton « { $name } » ? L'appareil perd immédiatement l'accès.
flash-forbidden = Vous n'avez pas l'autorisation de faire cela.
flash-not-found = Introuvable.
flash-exists = Cela existe déjà.
flash-bad-data = Les données ne sont pas valides : { $detail }
flash-failed = Cela n'a pas fonctionné – les détails figurent dans le journal du serveur.
flash-busy = Le serveur est occupé pour le moment – veuillez réessayer dans un instant.
err-etag-required = La version sur laquelle repose cette modification manque – veuillez recharger et enregistrer à nouveau.
err-input = Valeur non valide dans le champ « { $field } ».
err-input-body = La requête n'a pas pu être lue.
err-content-type = Ce type de contenu n'est pas pris en charge.
err-too-large = Trop volumineux.
err-window-too-large = La période est trop longue.
err-too-many-changes = Trop de modifications à la fois.
err-colour-target = Cible de couleur inconnue.
err-task-calendar = Une tâche ne peut pas changer de calendrier.
err-too-long = Trop long.
err-push-disabled = Les notifications sont désactivées sur ce serveur.
err-push-https = Le point de terminaison doit être en https.
err-push-endpoint = Ce n'est pas une adresse de notification d'un navigateur.
err-push-keys = Les clés de l'abonnement aux notifications ne sont pas valides.
err-vacation-title = { vacation-title-bad }: { $detail }
err-snap-point = Point d'accroche illisible : { $token }
err-color = La couleur doit être au format #rrggbb.
err-color-or-empty = La couleur doit être au format #rrggbb ou vide.
err-name-required = Indiquez un nom.
err-summary-required = Indiquez un titre.
err-end-before-start = La fin précède le début.
err-due-before-start = L'échéance précède le début.
err-changed-contact = Le contact a été modifié ailleurs – rechargez la page.
err-changed-event = L'événement a été modifié ailleurs – rechargez la page.
err-changed-task = La tâche a été modifiée ailleurs – rechargez la page.
err-undo-nothing = L'annulation n'a rien changé.
err-attach-base64 = La pièce jointe n'est pas encodée en base64.
err-attach-too-large = Pièce jointe trop volumineuse : { $size } octets (limite { $file } par fichier, { $total } par enregistrement).
err-too-many-presets = Trop de rappels prédéfinis.
err-invalid-preset = Rappel prédéfini non valide.
err-rrule-empty = La règle de répétition est vide.
err-rrule-freq = La règle de répétition doit commencer par FREQ=.
err-rrule = Règle de répétition non valide : { $detail }
err-latlon = Latitude/longitude hors limites.
err-ics = Le fichier .ics n'est pas valide : { $detail }
err-no-calendar-objects = Le fichier ne contient aucun événement, tâche ni entrée de journal.
err-vcf = Le fichier .vcf n'est pas valide : { $detail }
err-no-vcards = Le fichier ne contient aucune vCard.
err-date-range = La date est hors des années 1 à 9999.
err-alarm-range = Le rappel est trop éloigné de l'événement (400 ans au plus).
err-import-too-large = Le fichier est trop volumineux : { $size } octets (limite { $limit }).
err-import-too-many = Le fichier contient trop d'entrées : { $count } (limite { $limit }).
err-subscribed-read-only = Un calendrier abonné est en lecture seule.
err-no-event-or-task = Cet objet ne contient ni événement ni tâche.
err-rule-mode = Mode de règle inconnu { $mode }.
err-rule-title = Titre non valide.
err-rule-colour = Couleur non valide.
err-rule-entries = Trop d'entrées dans la règle.
err-rule-files = Une règle ne peut pas intégrer de fichiers.
err-category-name = Nom de catégorie non valide.
err-rule-scope = Portée de règle inconnue.
err-group-empty-id = Ce nom de groupe ne donne aucun identifiant.
err-group-exists = Le groupe { $id } existe déjà.
err-group-individuals = Seules des personnes peuvent rejoindre un groupe.
err-group-confirm = Le nom saisi ne correspond pas.
err-group-gone = Le groupe n'existe plus.
flash-confirm-mismatch = Le nom saisi ne correspond pas.
flash-group-trashed = Groupe mis à la corbeille.
flash-saved = Enregistré.
filter-holidays = Jours fériés et vacances
filter-public = Jours fériés
filter-school = Vacances scolaires
filter-mine = Uniquement les miens
filter-categories = Catégories
pick-date = Choisir une date
today = Aujourd'hui
nav-prev = Précédent
nav-next = Suivant
cancel = Annuler
view-day = Jour
view-week = Semaine
view-month = Mois
view-year = Année
view-more = Plus
view-weekends = Week-ends libres
view-vacation = Planning des congés
view-countdown = Mur des comptes à rebours
view-numbers = L'année en chiffres
ed-dayline = Afficher/masquer la ligne du jour
ed-title = Titre
ed-location = Lieu
ed-calendar = Calendrier
ed-date = Date
ed-from = De
ed-to = À
ed-end-date = Fin (date)
ed-allday = Toute la journée
ed-reminder-0 = Au début
ed-repeat-never = Jamais
ed-repeat-daily = Tous les jours
ed-repeat-weekly = Toutes les semaines
ed-repeat-monthly = Tous les mois
ed-repeat-yearly = Tous les ans
ed-repeat-custom = Règle personnalisée …
ed-card-when = Quand
ed-card-where = Lieu
ed-card-alarm = Rappel
ed-card-repeat = Répétition
ed-card-cats = Catégories
ed-card-note = Notes
ed-add-card = Ajouter une carte
ed-unlock = Modifier
ed-view-hint = Mode lecture · ouvrez le cadenas pour modifier
ed-create = Créer l'événement
ed-reset = Réinitialiser
ed-close = Fermer
ed-cat-add = Ajouter une catégorie …
ed-title-new = Créer un événement
ed-title-new-task = Créer une tâche
ed-tpl-head = Reprendre d’un événement passé
ed-tpl-count = { $n }×
ed-tpl-last = dernier { $date }
ed-series-hint = Événement récurrent — les modifications s'appliquent à toute la série.
ed-card-status = Indicateurs
ed-status = Statut
ed-st-confirmed = confirmé
ed-st-tentative = provisoire
ed-st-cancelled = annulé
ed-transp = Afficher comme
ed-transp-opaque = occupé
ed-transp-transparent = disponible
ed-class = Visibilité
ed-class-public = public
ed-class-private = privé
ed-class-confidential = confidentiel
ed-priority = Priorité
ed-prio-high = haute
ed-prio-medium = moyenne
ed-prio-low = basse
ed-unset = non défini
ed-transp-default = par défaut : occupé
ed-class-default = par défaut : public
ed-prio-none = aucune
ed-view-opaque = affiché comme occupé
ed-view-transparent = affiché comme disponible
ed-view-prio = priorité { $n }
ed-card-links = Liens et pièces jointes
ed-link-url = Site web
ed-link-attach = Pièce jointe
ed-link-conf = Conférence
ed-link-label = Libellé
ed-embedded = intégré
ed-lk-paste = Collez un lien – son type est détecté
ed-lk-addbtn = Ajouter
ed-lk-search = Rechercher un événement ou une tâche …
ed-lk-drop = Déposez des fichiers ici ou cliquez
ed-lk-drop-sub = jusqu'à 2 Mo intégrés, au-delà sous forme de lien cloud
ed-lk-toobig = { $name } est trop volumineux (max. 2 Mo) – enregistrez-le comme lien cloud
ed-lk-total = intégré : { $n } sur 6 Mo
ed-lk-conf = Conférence
ed-lk-internal = Dans ce calendrier
ed-lk-internet = Sur Internet
ed-lk-join = Rejoindre
ed-lk-meeting = Réunion { $provider }
ed-lk-link = Lien
ed-lk-id = ID
ed-lk-pwd = mot de passe dans le lien
ed-lk-phone = Téléphone
ed-lk-jitsi = Créer une salle Jitsi
ed-lk-edit = Modifier
ed-lk-remove = Retirer
ed-lk-address = Adresse
ed-lk-notfound = introuvable
ed-lk-task = Tâche
ed-lk-event = Événement
ed-lk-done = terminée
ed-lk-download = Télécharger
ed-lk-nothing = Aucun résultat
ed-nt-to-people = → Personnes
ed-nt-to-links = → Liens
ed-nt-to-cats = → Catégories
ed-nt-call = appeler
ed-nt-mail = écrire
ed-nt-contact = Contact
ed-nt-cat = Catégorie
ed-ct-existing = existante
ed-ct-new = créer « { $n } »
ed-ct-n = { $n }
ed-ct-rule = Règles
ed-ct-norule = aucune règle
ed-ct-lock = Verrou
ed-ct-default = Modèle
ed-ct-over = remplacé par { $n }
ed-ct-up = monter
ed-ct-down = descendre
ed-ct-newonly = s'applique aux nouveaux événements
ed-ct-changed = modifié
ed-ct-no = non
ed-ct-fields = Toute la journée|Calendrier|Couleur|Lieu|Rappel|Répétition|Statut|Afficher comme|Visibilité|Priorité|Liens|Titre
ed-ct-any = quelconque
ed-ct-rulemode = quelconque (hors règle), modèle (prérempli, modifiable) ou verrou (imposé)
ed-ct-rule-title = Règle pour la catégorie { $name }
ed-lk-save-first = Enregistrez d'abord, puis ouvrez
ed-color = Couleur
ed-color-cal = couleur du calendrier
ed-geo = Coordonnées
ed-geo-map = Choisir la position sur une carte
gm-title = Choisir une position
gm-none = Aucune adresse trouvée
gm-world = Carte du monde : faites glisser et zoomez jusqu'à la position, puis appliquez.
gm-error = Carte ou recherche indisponible.
ed-address = Adresse
gm-close = Fermer
gm-back = Retour
ed-unsaved = Les modifications non enregistrées seront perdues.
ed-discard = Ignorer
ed-loc-swap = Échanger adresse et coordonnées
ed-loc-add-coords = Coordonnées
ed-loc-add-addr = Adresse
ed-loc-rm-coords = Retirer les coordonnées
ed-loc-rm-addr = Retirer l'adresse
ed-err-coords = Aucune coordonnée trouvée pour cette adresse
ed-err-addr = Aucune adresse trouvée à cette position
ed-err-invalid = Coordonnées non valides (latitude, longitude)
ed-coords-hint = latitude, longitude
gm-determine = Trouver l'adresse
gm-apply = Appliquer
ed-card-people = Personnes
ed-add-alarm = Rappel
ed-exdates = Exceptions
ed-rdates = Dates supplémentaires
ed-every = tous les
ed-unit-daily = jours
ed-unit-weekly = semaines
ed-unit-monthly = mois
ed-unit-yearly = ans
ed-end = Fin
ed-end-never = jamais
ed-end-count = après un nombre
ed-end-until = à une date
ed-times = fois
ed-rr-freqs = Jamais|Quot.|Hebdo.|Mens.|Annuel
ed-rr-custom-short = Personnalisée
ed-rr-custom-hint = Cette règle contient des éléments que le calendrier ne peut pas afficher. Elle est conservée sous forme de texte.
ed-rr-each = chaque jour|chaque semaine|chaque mois|chaque année
ed-rr-in = en
ed-rr-ord = premier|deuxième|troisième|quatrième|dernier
ed-rr-interval = Intervalle
ed-rr-no-end = sans fin
ed-rr-count-q = Combien de fois au total ?
ed-rr-int-q = Tous les combien de { $u } ?
ed-rr-n = tous les n …
ed-rr-hint-pattern = Clic sur un jour = active ou désactive ce jour de la semaine ou du mois
ed-rr-hint-ex = Clic sur une occurrence = exception · sur un jour libre = date supplémentaire
ed-rr-lg-occ = Occurrence
ed-rr-lg-ex = Exception
ed-rr-lg-rd = Date supplémentaire
ed-rr-lg-start = Début
ed-rr-lg-end = Fin
ed-rr-pick = Choisissez la date de fin : cliquez sur le dernier jour de la série dans le calendrier.
ed-rr-mode = Clic dans le calendrier
ed-rr-mode-pattern = Motif
ed-rr-mode-ex = Exceptions
ed-before-start = avant le début
ed-before-end = avant la fin
ed-at-time = à une heure précise
ed-act-display = Affichage
ed-act-audio = Son
ed-act-email = E-mail
ed-role-organizer = Organisateur
ed-role-req = Participant
ed-role-opt = Facultatif
ed-role-chair = Président
ed-role-non = Pour information
ed-status-accepted = accepté
ed-status-declined = refusé
ed-status-tentative = provisoire
ed-status-needs = sans réponse
ed-status-delegated = délégué
ed-cu-group = Groupe
ed-cu-room = Salle
ed-cu-resource = Ressource
ed-pe-org-short = org.
ed-pe-opt-short = fac.
ed-pe-book = Ouvrir dans le carnet d'adresses
ed-pe-book-n = { $n } contacts similaires
ed-pe-pick = Quel contact ?
ed-pe-role = Rôle
ed-pe-reply = Réponse
ed-pe-rsvp = Réponse demandée
ed-pe-yes = oui
ed-pe-from = remplace { $n }
ed-pe-to = représenté par { $n }
ed-pe-known = dans le carnet d'adresses
ed-pe-unknown = absent du carnet d'adresses
ed-h-count = { $n } fois
ed-h-until = jusqu'au
ed-h-on = le
ed-h-every = tous les { $n }
ed-h-min = { $n } min
ed-h-before = avant
ed-h-hours = { $n } h
ed-h-days = { $n ->
    [one] { $n } jour
   *[other] { $n } jours
}
ed-h-repeat = répété { $n }×
ed-h-weeks = { $n ->
    [one] { $n } semaine
   *[other] { $n } semaines
}
ed-h-after = après le début
ed-h-after-end = après la fin
ed-h-at-start = au début
ed-h-at-end = à la fin
ed-h-repeat-every = tous les
ed-h-to = à
ed-after-start = après le début
ed-after-end = après la fin
ed-u-minutes = minutes
ed-u-hours = heures
ed-u-days = jours
ed-u-weeks = semaines
ed-u-months = mois
ed-al-when = Quand
ed-al-kind = Type
ed-al-text = Texte
ed-al-text-hint = sinon le titre de l'événement
ed-al-subject = Objet
ed-al-body = Message
ed-al-to = À
ed-al-to-hint = adresses e-mail, séparées par des virgules
ed-al-sound = Fichier son
ed-al-sound-hint = vide = son par défaut
ed-al-repeat = Répéter
ed-al-prox-arrive = à l'arrivée
ed-al-prox-depart = au départ
ed-al-prox-connect = voiture connectée
ed-al-prox-disconnect = voiture déconnectée
ed-al-preset = Préréglage
ed-al-preset-add = Enregistrer comme préréglage
ed-al-preset-rm = Retirer le préréglage
ed-al-preset-hint = Les préréglages s'appliquent à tous vos événements (sans le texte)
ed-al-preset-at = heure fixe – ne peut pas être un préréglage
ed-al-presets = Préréglages
ed-al-settings = Réglages
ed-al-event = Événement
ed-al-missing = heure manquante
ed-al-to-missing = destinataire manquant
ct-rail = Accès rapide
ct-list = Liste
ct-table = Tableau
ct-columns = Colonnes :
ct-show = Afficher :
col-phone = Téléphone
col-email = E-mail
col-category = Catégorie
col-addressbook = Carnet d'adresses
col-birthday = Anniversaire
col-address = Adresse
col-note = Note
col-org = Entreprise
col-open = Ouvrir
ct-none = Aucun contact pour l'instant. Créez un carnet d'adresses ou importez un fichier .vcf dans les réglages.
ct-new-row = Nouveau contact …
ct-given = Prénom
ct-family = Nom
ct-only-me = moi uniquement
tasks-open = en cours
vacation-rule = Règle de décompte des congés
vacation-rule-hint = Catégorie : les événements portant cette catégorie sont comptés. Calendrier : tous les événements du calendrier. Titre : les événements dont le titre correspond à l'expression régulière (sans tenir compte de la casse). Seuls les jours ouvrés hors jours fériés sont comptés.
vacation-kind-category = Catégorie
vacation-kind-calendar = Calendrier
vacation-kind-title = Titre
vacation-kind-legacy = Catégorie ou titre
vacation-title-bad = Expression de titre invalide
push-title = Notifications du navigateur
push-hint = Les rappels d'événements (VALARM) arrivent comme notifications Push sur cet appareil — même onglet fermé.
push-enable = Activer sur cet appareil
push-disable = Désactiver sur cet appareil
push-unavailable = Push n'est pas configuré sur ce serveur (pas de clé VAPID).
push-devices = Appareils enregistrés
ct-display-name = Nom affiché
ct-new-title = Créer un contact
ct-name-hint = Le nom affiché suit le prénom et le nom jusqu'à ce que vous le modifiiez vous-même
ct-nickname = Surnom
ct-prefix = Titre / préfixe
ct-additional = Autres prénoms
ct-suffix = Suffixe
ct-more-names = Plus de champs de nom
ct-org = Organisation
ct-dept = Service
ct-title = Fonction
ct-street = Rue et numéro
ct-code = Code postal
ct-city = Ville
ct-country = Pays
ct-bday = Anniversaire
ct-anniv = Date anniversaire
ct-date-label = Libellé, p. ex. fête
ct-year-optional = année facultative
ct-card-name = Nom
ct-card-tel = Téléphone
ct-card-email = E-mail
ct-card-adr = Adresses
ct-card-web = En ligne
ct-card-im = Messageries
ct-card-work = Travail
ct-card-dates = Dates
ct-card-rel = Relations
ct-card-cats = Catégories
ct-card-note = Note
ct-add-tel = Téléphone
ct-add-email = E-mail
ct-add-adr = Adresse
ct-add-url = Site web / profil
ct-add-im = Messagerie
ct-add-date = Autre date
ct-add-rel = Relation
ct-add-card = Ajouter une carte
ct-remove-card = Retirer la carte (supprime ses données)
ct-remove-card-confirm = Retirer cette carte ? Ses données seront supprimées.
ct-cat-add = Ajouter une catégorie …
ct-value = Valeur
ct-pref = préféré
ct-remove = retirer
ct-unlock = Modifier
ct-view-hint = Mode lecture · ouvrez le cadenas pour modifier
ct-create = Créer le contact
ct-reset = Réinitialiser
ct-back = Retour au nouveau contact
ct-vcf = Télécharger la vCard
ct-days-today = aujourd'hui !
ct-days-in = { $n ->
    [one] dans { $n } jour
   *[other] dans { $n } jours
}
ct-turns = aura { $n } ans
act-call = Appeler
act-sms = SMS
act-wa = WhatsApp
act-copy = Copier
act-copied = copié
act-mail = Écrire un e-mail
act-open = Ouvrir dans le navigateur
act-chat = Ouvrir la discussion
act-goto = Aller au contact
act-goto-missing = Aucun contact portant ce nom dans le carnet d'adresses.
act-osm = OpenStreetMap
act-gmaps = Google Maps
kind-cell = Mobile
kind-home = Domicile
kind-work = Travail
kind-fax = Fax
kind-other = Autre
url-website = Site web
url-mastodon = Mastodon
url-linkedin = LinkedIn
url-instagram = Instagram
url-other = Autre
rel-spouse = Conjoint(e)
rel-child = Enfant
rel-parent = Parent
rel-sibling = Frère/sœur
rel-friend = Ami(e)
rel-colleague = Collègue
rel-contact = Autre

## Task editor (shares its cards with the event editor, see templates/editor.html)
ed-card-general = Général
ed-card-progress = Avancement
ed-start = Début
ed-due = Échéance
ed-percent = Avancement
ed-completed-at = Terminée le
ed-tst-open = À faire
ed-tst-doing = En cours
ed-tst-done = Terminée
ed-tst-cancelled = Annulée
ed-view-pct = { $n } % effectué
ed-view-done = terminée le { $at }
cat-scope-events = Événements
cat-scope-tasks = Tâches

## Task views
tk-back = Afficher les terminées
tk-back-hint = Jusqu'où remonter pour afficher les tâches terminées
tk-back-day = Aujourd'hui
tk-back-week = Cette semaine
tk-back-month = Ce mois-ci
tk-back-quarter = Ce trimestre
tk-back-year = Cette année
tk-back-all = Sans limite
tk-v-groups = Groupes
tk-v-table = Tableau
tk-v-board = Tableau kanban
tk-v-timeline = Chronologie
tk-v-deck = Cartes
tk-v-history = Historique
tk-columns = Colonnes
tk-col-created = Créée
set-account-sub = Compte, jetons d'app
set-vacation = Congés
set-days = { $n ->
    [one] { $n } jour
   *[other] { $n } jours
}
set-snaps = { $n ->
    [one] { $n } point d'accroche
   *[other] { $n } points d'accroche
}
set-data = Données
set-push-here = Dans ce navigateur
set-export-ics = Exporter (.ics)
set-export-vcf = Exporter (.vcf)
set-import = Importer …
set-new-calendar = Nouveau calendrier
set-new-addressbook = Nouveau carnet d'adresses
set-comp-event = Événements
set-comp-todo = Tâches
set-comp-journal = Journal
set-rule = Règle
cat-count-task = { $n ->
    [one] { $n } tâche
   *[other] { $n } tâches
}
set-region = Région
set-regions-public = Jours fériés
set-regions-school = Vacances scolaires
set-region-add = Ajouter une région
set-regions-none = Aucune
set-regions-n = { $n } régions
set-regions-public-hint = La première région est la vôtre : les jours de congé et les ponts se comptent d’après elle. Les jours fériés qui ne valent pas dans toutes les régions portent le code de leurs régions, par ex. « Buß- und Bettag (SN) ».
set-regions-school-hint = Les vacances qui ne valent pas dans toutes les régions portent le code de leurs régions, par ex. « Winterferien (BY) ».
set-tz-default = Par défaut ({ $tz })
set-colors = Couleurs
set-colors-hint = Chaque élément doté d’une couleur figure ici. Le sélecteur propose exactement ces couleurs, chacune nommée d’après sa source ; ＋ en ajoute une nouvelle. La couleur d’une catégorie est modifiée dans sa règle.
set-sources = Jours fériés : { $public } · Vacances scolaires : { $school }
set-src-none = aucune source
set-pick-search = Rechercher

## server-rendered views (render.rs, more.rs, calendar.rs)
v-after = après
v-all-day = toute la journée
v-at-start = au début
v-attachment = Pièce jointe
v-before = avant
v-birthdays-in-the-address-books = { $n ->
    [one] anniversaire dans les carnets d'adresses
   *[other] anniversaires dans les carnets d'adresses
}
v-bridge-days = Ponts
v-busiest-weeks = Semaines les plus chargées
v-busy-weekend-days = { $n ->
    [one] jour de week-end occupé
   *[other] jours de week-end occupés
}
v-cancelled = Annulé
v-coming-up = À venir
v-confidential = Confidentiel
v-copy-address = Copier l'adresse
v-copy-coordinates = Copier les coordonnées
v-counting-rule = Règle de décompte :
v-daily = Tous les jours
v-day-off = jour de congé
v-days = { $n ->
    [one] jour
   *[other] jours
}
v-days-in-a-row = jours d'affilée
v-density = Densité
v-shown-free = Affiché comme disponible
v-entitlement-and-rule-in-the-settings = Droits et règle dans les réglages.
v-evenings-booked-from-6-pm = { $n ->
    [one] soirée prise (à partir de 18 h)
   *[other] soirées prises (à partir de 18 h)
}
v-n-events = { $n ->
    [one] { $n } événement
   *[other] { $n } événements
}
v-events-per-calendar = Événements par calendrier
v-events-this-year = { $n ->
    [one] événement cette année
   *[other] événements cette année
}
v-extended-by-a-holiday = { $n ->
    [one] prolongé par un jour férié
   *[other] prolongés par un jour férié
}
v-free = libre
v-free-weekends = { $n ->
    [one] week-end libre
   *[other] week-ends libres
}
v-high-priority = Priorité haute
v-holidays = Jours fériés
v-just-passed = Tout juste passé
v-last = derniers
v-load-per-week = Charge par semaine
v-long-weekend = week-end prolongé
v-low-priority = Priorité basse
v-medium-priority = Priorité moyenne
v-monthly = Tous les mois
v-more = de plus
v-new-year = Nouvel an
v-no-bridge-days-this-year = Aucun pont cette année.
v-no-events = Aucun événement
v-no-events-on-this-day = Aucun événement ce jour-là.
v-not-congratulated-yet = pas encore félicité ?
v-over = terminé
v-planned = { $n ->
    [one] prévu
   *[other] prévus
}
v-print-ctrl-p = Imprimer : Ctrl+P
v-private = Privé
v-public-holiday = Jour férié
v-recurring = Récurrent
v-remaining-of = { $n ->
    [one] restant sur
   *[other] restants sur
}
v-school-holidays = Vacances scolaires
v-tentative = Provisoire
v-today = Aujourd'hui
v-top-right-on-mondays = en haut à droite le lundi
v-trips = { $n ->
    [one] voyage
   *[other] voyages
}
v-up-to-30-days = jusqu'à 30 jours
v-vacation = Congés
v-vacation-days-planned = { $n ->
    [one] jour de congé prévu
   *[other] jours de congé prévus
}
v-was = était
v-week-abbr = Sem.
v-weekend = Week-end
v-weekends = Week-ends
v-weekly = Toutes les semaines
v-year-planner = Planning annuel
v-yearly = Tous les ans
mon-1 = janvier
mon-2 = février
mon-3 = mars
mon-4 = avril
mon-5 = mai
mon-6 = juin
mon-7 = juillet
mon-8 = août
mon-9 = septembre
mon-10 = octobre
mon-11 = novembre
mon-12 = décembre
dow-1 = lun
dow-2 = mar
dow-3 = mer
dow-4 = jeu
dow-5 = ven
dow-6 = sam
dow-7 = dim
dowl-1 = lundi
dowl-2 = mardi
dowl-3 = mercredi
dowl-4 = jeudi
dowl-5 = vendredi
dowl-6 = samedi
dowl-7 = dimanche
date-long = { $dow } { $day } { $month }
date-dm = { $d }/{ $m }
date-fmt = %d/%m/%Y
date-day-title = { $day } { $month } { $year }
date-week = Semaine { $week } { $year }
date-week-short = S{ $week }
v-rr-every-days = Tous les { $n } jours
v-rr-every-weeks = Toutes les { $n } semaines
v-rr-every-months = Tous les { $n } mois
v-rr-every-years = Tous les { $n } ans
v-rr-on-days = le { $days }
v-rr-ordinal = { $n ->
    [1] 1er
   *[other] { $n }e
}
v-rr-on-nth = le { $which } { $day }
v-rr-on-monthday = le { $d }
v-rr-on-date = le { $d } { $month }
v-rr-in-month = en { $month }
v-rr-count = { $n ->
   *[other] { $n } fois
}
v-rr-until = jusqu'au { $date }
v-n-days = { $n ->
    [one] { $n } jour
   *[other] { $n } jours
}
v-n-hours = { $n ->
    [one] { $n } heure
   *[other] { $n } heures
}
v-n-minutes = { $n ->
    [one] { $n } minute
   *[other] { $n } minutes
}

## app.js (shipped as JSON by layout.html; placeholders are %name, not Fluent variables)
js-yes = Oui
js-no = Non
js-close = Fermer
js-copied = Copié.
js-req-offline = Pas de connexion au serveur.
js-req-offline-bar = Hors ligne – les modifications ne peuvent pas être enregistrées pour l’instant.
js-req-auth = La session a expiré.
js-req-forbidden = Vous n’avez pas l’autorisation de faire cela.
js-req-notfound = Introuvable – peut-être supprimé ailleurs.
js-req-conflict = Modifié ailleurs entre-temps.
js-req-invalid = La saisie n’a pas été acceptée.
js-req-busy = Le serveur est occupé – réessayez dans un instant.
js-req-server = Le serveur a signalé une erreur – réessayez plus tard.
js-req-conflict-ask = Ceci a été modifié ailleurs entre-temps. Enregistrer votre version par-dessus ou charger l’autre ? (Échap : continuer la modification)
js-req-mine = Enregistrer ma version
js-req-theirs = Charger l’autre (abandonner mes modifications)
js-req-restored = Vos modifications non enregistrées d’avant la connexion sont de retour – vérifiez-les et enregistrez-les.
js-req-copy = La copie dans le presse-papiers n’a pas fonctionné.
js-week = Semaine %w %y
js-week-short = S%w
js-ed-suggNone = Aucune suggestion
js-ed-saved = enregistré
js-ed-err = Erreur
js-ed-del = Supprimer cet événement ?
js-ed-delRule = Supprimer cette règle ? Les événements conservent leurs valeurs.
js-ed-series = Événement récurrent : toute la série est modifiée.
js-ed-newEv = Nouvel événement
js-ed-newTask = Nouvelle tâche
js-ed-editEv = Modifier l'événement
js-ed-conflict = Modifié ailleurs — rechargez.
js-ed-copied = Copié
js-ed-delTask = Supprimer cette tâche ?
js-search-jump = Aller à
js-search-ev = Événements
js-search-ct = Contacts
js-search-tk = Tâches
js-search-none = Aucun résultat
js-ct-del = Supprimer ce contact ?
js-ct-err = Erreur
js-ct-phone = Téléphone
js-ct-email = E-mail
js-ct-create = Créer le contact
js-ct-save = Enregistrer
js-ct-reset = Réinitialiser
js-ct-cancel = Annuler
js-ct-delete = Supprimer
js-ctsort-given = Prénom
js-ctsort-family = Nom
js-ctsort-bday = Anniversaire
js-ctsort-email = E-mail
js-ctsort-none = Non renseigné
js-sort-due = Échéance
js-sort-done = Terminée le
js-sort-prio = Priorité
js-sort-pct = Avancement
js-sort-title = Titre
js-sort-cal = Calendrier
js-sort-created = Création
js-set-err = Erreur
js-set-active = Actif sur cet appareil.
js-set-denied = Les notifications ont été refusées dans le navigateur.
js-set-unsupported = Ce navigateur ne prend pas en charge les notifications Push.
js-set-imported = { $n ->
    [one] %n objet importé.
   *[other] %n objets importés.
}
js-tk-open = À faire
js-tk-doing = En cours
js-tk-done = Terminée
js-tk-cancelled = Annulée
js-tk-nodate = Sans date
js-tk-overdue = En retard
js-tk-today = Aujourd'hui
js-tk-week = Cette semaine
js-tk-later = Plus tard
js-tk-del = Supprimer cette tâche ?
js-tk-err = Erreur
js-tk-sort = Trier
js-tk-askDone = Marquer « %t » comme terminée ?
js-tk-yes = Oui
js-tk-no = Non
js-tk-asc = Croissant
js-tk-desc = Décroissant
js-tk-done_at = terminée
js-tk-due = échéance
js-tk-wasdue = échéance passée
js-tk-empty = Rien ici.
js-tk-prio = Priorité
js-tk-pct = % effectué
js-tk-count = sur
js-tk-keys = J/K parcourir · Espace termine · E modifie
js-tk-hist = plus foncé = plus de tâches terminées
js-tk-histNone = Rien n'a été terminé ce jour-là.
js-tk-nodateBox = Sans date
js-tk-loading = chargement …
js-tk-start = Début
js-tk-cats = Catégories
js-tk-edit = Modifier
js-pick-country = Pays
js-pick-tz = Fuseau horaire
js-pick-whole = Tout le pays
js-pick-all = Tout %name
js-pick-default = Par défaut (%tz)
js-pick-none = Aucun résultat
js-region-remove = Retirer
js-region-first = En faire votre région
js-region-own = La vôtre
js-regions-none = Aucune
js-regions-n = %n régions
js-snp-min = %n min
js-snp-h = %n h
js-snp-d = { $n ->
    [one] %n jour
   *[other] %n jours
}
js-snp-w = { $n ->
    [one] %n semaine
   *[other] %n semaines
}
js-snp-u-min = min
js-snp-u-h = h
js-snp-u-d = jours
js-snp-u-w = semaines
js-snp-start = avant le début
js-snp-end = avant la fin
js-snp-begin = Début
js-snp-finish = Fin
js-snp-change = Modifier
js-snp-remove = Supprimer
js-snp-close = Fermer
js-snp-add = Ajouter %what
js-snp-amount = Nombre
js-snp-bad = Saisissez un nombre supérieur à 0
js-snp-max = 366 jours au maximum
js-snp-dup = Ce point d'accroche existe déjà
js-snp-last = Un point avant le début doit rester
js-snaps-n = { $n ->
    [one] %n point d'accroche
   *[other] %n points d'accroche
}
js-imp-file = Fichier « %file »
js-imp-ev = { $n ->
    [one] %n événement
   *[other] %n événements
}
js-imp-td = { $n ->
    [one] %n tâche
   *[other] %n tâches
}
js-imp-jr = { $n ->
    [one] %n entrée de journal
   *[other] %n entrées de journal
}
js-imp-taken = repris du fichier : %what
js-imp-f-name = nom
js-imp-f-desc = description
js-imp-f-color = couleur
js-imp-f-tz = fuseau horaire
js-imp-empty = Le fichier ne contient ni événements, ni tâches, ni entrées de journal.
js-imp-other = Choisir un autre fichier …
js-imp-in-file = Contenu dans le fichier
js-imp-busy = Import en cours …
js-imp-done = { $count ->
    [one] « %title » créé · %count entrée importée
   *[other] « %title » créé · %count entrées importées
}
js-imp-ct = { $n ->
    [one] %n contact
   *[other] %n contacts
}
js-imp-gr = { $n ->
    [one] %n groupe
   *[other] %n groupes
}
js-imp-ab-empty = Le fichier ne contient aucun contact.
js-vac-word = Mot entier
js-vac-begin = Au début
js-vac-end = À la fin
js-vac-or = ou
js-vac-any = n'importe quoi
js-vac-loading = Chargement des titres …
js-vac-empty = Saisissez un motif – vous verrez ici quels titres de vos événements correspondent.
js-vac-hits = %hits titres sur %all correspondent · %events événements
js-vac-none = Aucun titre ne correspond.
js-vac-bad = Expression invalide : %msg
js-vac-unsup = Les assertions avant/arrière et les références arrière ne sont pas prises en charge.
js-vac-match = Correspond
js-vac-miss = Ne correspond pas
js-vac-more = … et %n de plus
js-vac-no-events = aucun événement
js-color-k-calendar = Agenda
js-color-k-addressbook = Carnet d’adresses
js-color-k-event = Événement
js-color-k-task = Tâche
js-color-k-category = Catégorie
js-color-k-category-task = Catégorie de tâches
js-color-k-holiday = Jours fériés
js-color-k-school = Vacances scolaires
js-color-src = %kind « %name »
js-color-more = { $n ->
    [one] + %n autre
   *[other] + %n autres
}
js-color-count = { $n ->
    [one] %n entrée
   *[other] %n entrées
}
js-color-changed = Recolorés : %n.
js-color-recolor-event = Événements existants de la catégorie « %cat » avec l’ancienne couleur : %n. Leur donner la nouvelle couleur ?
js-color-recolor-task = Tâches existantes de la catégorie « %cat » avec l’ancienne couleur : %n. Leur donner la nouvelle couleur ?
js-color-recolor-yes = Recolorer
js-color-recolor-no = Seulement la règle
js-color-recolored = Recolorés : %n.
js-color-std = Par défaut
js-color-own = Couleur propre
js-color-ro = abonnement, lecture seule
js-color-in-rule = Définie dans la règle
js-color-current = Couleur actuelle
js-color-add = Nouvelle couleur …
js-color-inherit-cal = Comme l’agenda – %cal
js-color-pv-allday = Toute la journée
js-color-on-light = sur clair %r : 1
js-color-on-dark = sur sombre %r : 1
js-color-new = Nouvelle couleur
js-color-for = pour %what
js-color-tab-fan = Nuancier
js-color-tab-field = Champ et curseurs
js-color-close = Fermer
js-color-apply = Appliquer
js-color-dist = Écart avec les couleurs utilisées
js-color-same = Presque identique à %src.
js-color-take = Prendre celle-ci
js-color-near = difficile à distinguer
js-color-similar = proche
js-color-prev = Carte précédente
js-color-next = Carte suivante
js-color-fan-tone = %fam · teinte %n sur 5
js-color-fan-near = Teinte la plus proche : %fam %n
js-color-text = Couleur en texte
js-color-dice = Couleur aléatoire et lisible
js-color-sugg = Suggestions
js-color-bad = Pas une couleur – p. ex. #2f6fed, tomato ou rgb(20 120 200)
js-color-m-simple = Simple
js-color-m-rgb = RVB
js-color-m-oklch = OKLCH
js-color-hue = Teinte
js-color-sat = Saturation
js-color-light = Luminosité
js-color-chroma = Chroma
js-color-red = Rouge
js-color-green = Vert
js-color-blue = Bleu
js-color-fam-red = Rouge
js-color-fam-coral = Corail
js-color-fam-orange = Orange
js-color-fam-mustard = Moutarde
js-color-fam-lime = Citron vert
js-color-fam-green = Vert
js-color-fam-teal = Pétrole
js-color-fam-sky = Ciel
js-color-fam-blue = Bleu
js-color-fam-indigo = Indigo
js-color-fam-violet = Violet
js-color-fam-berry = Baie
js-color-fam-earth = Terre
js-color-fam-slate = Ardoise
js-color-g-calendar = Agendas
js-color-g-addressbook = Carnets d’adresses
js-color-g-holiday = Jours fériés et vacances
js-color-g-category = Catégories · événements
js-color-g-category-task = Catégories · tâches
js-color-g-event = Événements avec leur propre couleur
js-color-g-task = Tâches avec leur propre couleur
js-color-g-offer = { $n ->
    [one] Dans le sélecteur · %n couleur
   *[other] Dans le sélecteur · %n couleurs
}
js-cfo-block-event = Calendrier et événements
js-cfo-block-task = Tâches
js-cfo-btn = Optimiser les couleurs
js-cfo-title = Optimiser les couleurs
js-cfo-sub-event = Calendriers, événements, leurs catégories, jours fériés et vacances – tout ce que la vue calendrier montre ensemble
js-cfo-sub-task = Tâches et leurs catégories – les couleurs des calendriers restent telles quelles
js-cfo-scope = Portée
js-cfo-rec = recommandé
js-cfo-l0 = Prudent
js-cfo-l0-text = Seules les entrées au titre identique et les événements d'une même catégorie reçoivent une couleur commune – la plus utilisée parmi eux. Rien n'est séparé.
js-cfo-l1 = Ranger
js-cfo-l1-text = Les titres de même racine (« Congé », « Congés ») et les abréviations (« AS » pour « Astreinte ») vont aussi ensemble. Les couleurs presque identiques de choses différentes sont séparées.
js-cfo-l2 = Ordonner
js-cfo-l2-text = La catégorie, le calendrier et les jours fériés donnent la couleur de leur famille, plus la majorité. Ce qui se distingue à peine est séparé.
js-cfo-l3 = Équilibré
js-cfo-l3-text = Comme « Ordonner », mais ce qui se ressemble seulement est aussi séparé. Moins de couleurs, toutes faciles à distinguer.
js-cfo-l4 = Approfondi
js-cfo-l4-text = Les séries de titres comme « Projet : … » forment aussi une famille, sauf si deux catégories s'y opposent. Séparation généreuse.
js-cfo-l5 = Nouvelle palette
js-cfo-l5-text = Tout ce que fait « Approfondi », et chaque famille reçoit une nouvelle couleur tirée de palettes professionnelles (Apple, Google Agenda), aussi éloignées que possible. Seuls les calendriers, les jours fériés et les vacances gardent la leur. Change le plus.
js-cfo-m-colors = Couleurs %a → %b
js-cfo-m-similar = Trop proches %a → %b
js-cfo-m-changes = Modifications : %n
js-cfo-manual = Modifié par vous : %n
js-cfo-discard = tout annuler
js-cfo-h-now = Éléments aujourd'hui
js-cfo-h-act = Action
js-cfo-h-after = Couleur après
js-cfo-help = À gauche, faites glisser un élément par sa poignée ⠿ vers le haut ou le bas : sur une autre action = l'y harmoniser, sur un séparateur entre deux actions = sa propre action « Séparer ». ⊘ exclut un élément. Touchez une couleur à droite pour la choisir vous-même. La case du milieu omet toute une action.
js-cfo-empty = Rien à améliorer à ce niveau.
js-cfo-empty-hint = Déplacez le curseur vers la droite pour plus de suggestions.
js-cfo-t-unify = Harmoniser
js-cfo-t-separate = Séparer
js-cfo-t-repaint = Nouvelle couleur
js-cfo-tag-split = séparé par vous
js-cfo-tag-moved = modifié par vous
js-cfo-tag-pin = votre couleur
js-cfo-man-moved = déplacé
js-cfo-man-split = séparé
js-cfo-on = ✓ Modifications : %n
js-cfo-none = ✓ rien à changer
js-cfo-off = omise – toucher
js-cfo-off-t = Omettre cette action
js-cfo-on-t = Refaire cette action
js-cfo-why-pin = choisie par vous
js-cfo-why-split = séparé par vous
js-cfo-why-std = couleur par défaut (%kind)
js-cfo-why-cal = couleur du calendrier « %name »
js-cfo-why-cat = couleur de la catégorie « %name »
js-cfo-why-most = couleur la plus utilisée (%n×)
js-cfo-why-pal = couleur de palette, loin des autres
js-cfo-why-same = nouvelle couleur : l'ancienne est la même que pour « %other »
js-cfo-why-near = nouvelle couleur : l'ancienne se distingue à peine de « %other » (%d)
js-cfo-why-none = tout est exclu
js-cfo-excl-state = exclu, reste tel quel
js-cfo-ro = abonné, lecture seule
js-cfo-stay = a déjà la couleur
js-cfo-grip = Faire glisser vers le haut ou le bas
js-cfo-unmove = Revenir à sa place
js-cfo-excl = Ne pas modifier cet élément
js-cfo-incl = inclure
js-cfo-incl-t = Inclure à nouveau
js-cfo-entries = Entrées : %n
js-cfo-new = nouveau
js-cfo-edit = modifier
js-cfo-unpin = Rétablir la suggestion
js-cfo-pick = Choisir une autre couleur
js-cfo-drop-join = Relâcher : harmoniser ici
js-cfo-drop-split = Relâcher : sa propre action « Séparer »
js-cfo-n-moved = À cause de votre couleur pour « %a », « %b » reçoit une nouvelle couleur.
js-cfo-n-same = Votre couleur pour « %a » est la même que pour « %b ».
js-cfo-n-near = Votre couleur pour « %a » se distingue à peine de « %b » (écart %d).
js-cfo-n-both = Vous avez choisi les deux couleurs vous-même.
js-cfo-n-cal = Les couleurs des calendriers ne cèdent pas – choisissez une autre couleur.
js-cfo-n-hol = Les couleurs des jours fériés et des vacances restent – choisissez une autre couleur.
js-cfo-n-fixed = Cette couleur reste telle quelle.
js-cfo-n-full = Il n'y a plus de place pour s'écarter.
js-cfo-n-excl = (exclu)
js-cfo-n-light = Votre couleur pour « %a » se voit mal sur fond clair.
js-cfo-n-dark = Votre couleur pour « %a » se voit mal sur fond sombre.
js-cfo-n-cats = Les catégories %list partageront une couleur.
js-cfo-n-cal-event = La couleur du calendrier « %a » change aussi sur la page des tâches.
js-cfo-n-cal-task = La couleur du calendrier « %a » change aussi dans la vue calendrier.
js-cfo-ask-level = Modifié à la main : %n (couleurs choisies, éléments déplacés ou exclus). Avec la portée « %level », tout est recalculé et ces modifications sont perdues.
js-cfo-ask-yes = Annuler et changer
js-cfo-ask-discard = Annuler toutes les modifications faites à la main ?
js-cfo-discard-yes = Annuler
js-cfo-apply = Appliquer
js-cfo-failed = Non modifiés : %n.

# top-bar search: help, history, all results
search-help = Aide à la recherche
search-help-words = Plusieurs termes affinent la recherche.
search-or = ou
search-help-scope = Chercher dans une seule catégorie :
search-scope-ev = événement
search-scope-ct = contact
search-scope-tk = tâche
search-help-date = Une date comme « 15/3 », « mars 2027 » ou « 2027 » y mène.
search-help-all = L’aperçu ne montre que les meilleurs résultats ; Entrée dans le champ les montre tous.
search-help-hist = Les recherches vont dans l’historique (Paramètres → Recherche) ; Tab reprend la suggestion grise et va à son résultat.
search-key-focus = champ de recherche
search-key-move = choisir
search-key-open = ouvrir / tout
search-key-tab = aller à la suggestion
search-key-esc = fermer
set-search = Recherche
set-search-off = Ne pas conserver
set-search-forever = Illimité
set-search-year = 1 an
set-search-hist = Historique de recherche
set-search-keep = Conserver
set-search-hint = Une recherche entre dans l’historique dès que vous ouvrez un résultat ou appuyez sur Entrée — avec le dernier résultat ouvert, vers lequel Tab mène directement. Les entrées plus anciennes s’effacent après la durée choisie.
set-search-entries = Entrées
set-search-clear = Effacer l’historique
set-search-empty = Aucune recherche conservée.
set-search-rm = Retirer
set-search-uses = cherché { $n }×
set-search-clear-ask = Effacer tout l’historique de recherche ?
js-search-all = Détails / Tous les résultats
js-search-of = %n sur %total
js-search-hist = Historique
js-search-histRm = Retirer de l’historique
js-search-dlg = Tous les résultats
js-search-tabAll = Tous
js-search-close = Fermer
js-search-keys = ↑ ↓ choisir · Entrée ouvrir · Échap fermer
js-search-only = Seulement %what
js-search-series = %n événements
js-search-why = Trouvé dans
js-search-f-title = Titre
js-search-f-location = Lieu
js-search-f-cats = Catégorie
js-search-f-person = Personne
js-search-f-email = E-mail
js-search-f-link = Lien
js-search-f-note = Note
js-search-f-related = Lié
js-search-f-status = Statut
js-search-f-geo = Coordonnées
js-search-f-coll = Collection
js-search-f-name = Nom
js-search-f-org = Organisation
js-search-f-job = Fonction
js-search-f-affix = Complément du nom
js-search-f-im = Messagerie
js-search-f-address = Adresse
js-search-f-web = Site web
js-search-f-rel = Relation
js-search-f-phone = Téléphone
js-search-f-bday = Anniversaire
js-search-f-anniv = Anniversaire de mariage
js-search-f-date = Date
js-search-f-due = Échéance
js-search-f-start = Début
js-search-f-done = Terminé le
js-search-k-word = mot entier
js-search-k-prefix = début de mot
js-search-k-joined = écrit attaché
js-search-k-chain = débuts de mots
js-search-k-inside = dans un mot
js-search-k-gaps = lettres omises
js-search-k-typo = proche de « %m »
js-search-k-phone = chiffres
js-search-k-date = date

# first start (js/onboarding/*.js): the overview and the hints that lead to each setting
js-onb-start-title = Premiers pas
js-onb-go-settings = Avant de pouvoir utiliser PractiCal, il manque encore quelques réglages.
js-onb-go-settings-b = Cliquez à tout moment sur cette roue dentée pour accéder aux paramètres du programme.
js-onb-back = Retour à tous les paramètres.
js-onb-back-b = Rien n'est ouvert ici. Le point suivant est au niveau supérieur.
js-onb-tz-title = Fuseau horaire
js-onb-tz-row = Définir votre fuseau horaire local.
js-onb-tz-row-b = Les heures des événements de l’agenda sont ainsi ajustées à votre heure locale.
js-onb-tz-act = Proposé : %tz
js-onb-tz-act-b = Trouvez votre fuseau dans la liste, ou confirmez la proposition. Tout événement sans fuseau propre sera ensuite compté dedans.
js-onb-regions-title = Jours fériés
js-onb-regions-row = Ajuster la région des jours fériés affichés.
js-onb-regions-none = N’afficher aucun jour férié
js-onb-tz-keep = Garder la proposition
js-onb-regions-row-b = Choisissez votre région pour que l’agenda affiche les jours fériés. Sans région, ils n’apparaissent pas.
js-onb-regions-add = Élargir les jours fériés affichés dans l’agenda.
js-onb-regions-add-b = Vous pouvez en ajouter plusieurs, si vous voulez voir les jours fériés de plusieurs régions.
js-onb-school-title = Vacances scolaires
js-onb-school-row = Ajuster la région des vacances scolaires affichées.
js-onb-school-row-b = Choisissez la région dont vous voulez voir les vacances — elle peut différer de celle des jours fériés. Sans région, aucune période de vacances n’est affichée.
js-onb-school-none = N’afficher aucune vacance scolaire
js-onb-done-title = Configuration terminée
js-onb-done-body = Tous les réglages de base sont faits. Vous retrouverez chacun d’eux là où je vous l’ai montré.
js-onb-done-go = C’est parti
js-onb-school-add = Élargir les vacances affichées dans l’agenda.
js-onb-school-add-b = Ici aussi, plusieurs sont possibles, si vous voulez voir les vacances de plusieurs régions.
js-onb-either = { $n ->
    [one] Un chemin est entouré.
   *[other] Les %n chemins sont entourés — prenez celui qui vous convient.
}
js-onb-calendar-title = Agenda
js-onb-calendar-row = Créer ou importer agendas et carnets d’adresses.
js-onb-calendar-row-b = Aucun n’existe encore — et sans agenda, il n’y a nulle part où créer un événement.
js-onb-calendar-ways = Trois chemins vers un agenda.
js-onb-calendar-ways-b = Créer si vous partez de zéro. Importer si vous abandonnez votre ancienne application d’agenda — les événements déménagent une fois. Synchroniser si vous voulez continuer à l’utiliser en parallèle — les deux côtés restent alors ensemble.
js-onb-addressbook-title = Carnet d'adresses
js-onb-addressbook-row = Créer ou importer agendas et carnets d’adresses.
js-onb-addressbook-row-b = Il manque encore un carnet d’adresses pour les contacts — sans lui, il n’y a nulle part où créer un contact.
js-onb-addressbook-ways = Trois chemins vers un carnet d’adresses.
js-onb-addressbook-ways-b = Créer si vous partez de zéro. Importer si vous abandonnez votre ancienne application de contacts — les contacts déménagent une fois. Synchroniser si vous voulez continuer à l’utiliser en parallèle.
js-onb-name = Donnez-lui un nom.
js-onb-name-b = Le nom apparaîtra dans la liste et sur chaque appareil. Couleur et composants sont préremplis.
js-onb-make = Tous les champs sont préremplis avec des valeurs par défaut.
js-onb-make-b = Vous pouvez tout changer, ou simplement créer et ajuster plus tard.
js-onb-import = Choisissez le fichier et importez-le.
js-onb-import-b = La collection est créée à partir du fichier ; vous pourrez la renommer ensuite.

## change history (/history): values and labels (domain::history_text), placeholders %name
hx-unit-min = min
hx-unit-h = h
hx-unit-d = jours
hx-unit-w = semaines
hx-al-before = %n avant
hx-al-after = %n après
hx-al-at-start = au début
hx-al-end = par rapport à la fin
hx-alarm = Rappel
hx-rr-daily = tous les jours
hx-rr-weekly = chaque semaine
hx-rr-monthly = chaque mois
hx-rr-yearly = chaque année
hx-rr-daily-n = tous les %n jours
hx-rr-weekly-n = toutes les %n semaines
hx-rr-monthly-n = tous les %n mois
hx-rr-yearly-n = tous les %n ans
hx-rr-until = jusqu’au %date
hx-prio-high = haute
hx-prio-mid = moyenne
hx-prio-low = basse
hx-v-tentative = provisoire
hx-v-confirmed = confirmé
hx-v-cancelled = annulé
hx-v-needs-action = à faire
hx-v-in-process = en cours
hx-v-completed = terminé
hx-v-public = public
hx-v-private = privé
hx-v-confidential = confidentiel
hx-v-opaque = occupé
hx-v-transparent = libre
hx-file = Fichier
hx-a-coll = Calendrier / carnet
hx-a-override = Occurrence du %date
hx-a-summary = Titre
hx-a-dtstart = Début
hx-a-dtend = Fin
hx-a-duration = Durée
hx-a-due = Échéance
hx-a-completed = Terminé le
hx-a-percent-complete = Avancement
hx-a-location = Lieu
hx-a-geo = Coordonnées
hx-a-description = Note
hx-a-valarm = Rappel
hx-a-rrule = Répétition
hx-a-exdate = Exceptions
hx-a-rdate = Dates en plus
hx-a-categories = Catégories
hx-a-color = Couleur
hx-a-attendee = Participants
hx-a-organizer = Organisateur
hx-a-status = Statut
hx-a-class = Visibilité
hx-a-transp = Occupé
hx-a-priority = Priorité
hx-a-url = Lien
hx-a-attach = Pièce jointe
hx-a-conference = Conférence
hx-a-related-to = Lié à
hx-a-fn = Nom
hx-a-n = Nom (parties)
hx-a-nickname = Surnom
hx-a-tel = Téléphone
hx-a-email = E-mail
hx-a-adr = Adresse
hx-a-bday = Anniversaire
hx-a-anniversary = Anniversaire de mariage
hx-a-org = Entreprise
hx-a-title = Fonction
hx-a-role = Rôle
hx-a-note = Note
hx-a-photo = Photo
hx-a-impp = Messagerie
hx-a-related = Relation
hx-a-acknowledged = Rappel acquitté
hx-a-dtstamp = Horodatage
hx-a-last-modified = Modifié le
hx-a-sequence = Révision
hx-a-created = Créé
hx-a-rev = Révision

hx-al-to = à %who
hx-al-repeat = répété %n×, toutes les %every
hx-prio-none = aucune
hx-v-action-display = Affichage
hx-v-action-audio = Son
hx-v-action-email = E-mail
hx-v-action-procedure = Procédure
hx-v-prox-arrive = à l’arrivée
hx-v-prox-depart = au départ
hx-v-prox-connect = à la connexion
hx-v-prox-disconnect = à la déconnexion
hx-v-accepted = accepté
hx-v-declined = refusé
hx-v-delegated = délégué
hx-v-chair = présidence
hx-v-req-participant = requis
hx-v-opt-participant = facultatif
hx-v-non-participant = pour information
hx-v-individual = personne
hx-v-group = groupe
hx-v-resource = ressource
hx-v-room = salle
hx-v-unknown = inconnu
hx-v-parent = parent
hx-v-child = enfant
hx-v-sibling = frère
hx-t-home = domicile
hx-t-work = travail
hx-t-cell = mobile
hx-t-fax = fax
hx-t-pager = bipeur
hx-t-text = SMS
hx-t-video = vidéo
hx-t-main = principal
hx-t-iphone = iPhone
hx-t-other = autre
hx-t-pref = préféré
hx-t-spouse = conjoint
hx-t-child = enfant
hx-t-parent = parent
hx-t-sibling = frère ou sœur
hx-t-friend = ami
hx-t-colleague = collègue
hx-a-x-alt-desc = Note (formatée, Outlook)
hx-a-styled-description = Note (formatée)
hx-a-image = Image
hx-a-contact = Contact
hx-a-resources = Ressources
hx-a-comment = Commentaire
hx-a-x-abdate = Autre date
hx-a-x-abrelatednames = Relation
hx-a-x-socialprofile = Profil
hx-a-gender = Genre
hx-a-kind = Type
hx-a-member = Membre
hx-a-source = Source
hx-a-lang = Langue
hx-a-tz = Fuseau horaire
hx-a-url-work = Lien
hx-a-deathdate = Date de décès
hx-a-logo = Logo
hx-a-label = Étiquette d’adresse
hx-a-prodid = Application
hx-a-version = Version du format
hx-a-x-microsoft-cdo-busystatus = Occupé (Outlook)
hx-a-x-apple-travel-advisory-behavior = Temps de trajet (Apple)
hx-a-x-apple-structured-location = Lieu avec coordonnées (Apple)
hx-a-vlocation = Lieu (bloc)
hx-a-participant = Participant (bloc)
hx-a-vresource = Ressource (bloc)

## change history page (history.js, shipped with its data)
hxj-title = Modifications
hxj-count = { $n ->
    [one] %n modification
   *[other] %n modifications
}
hxj-zoom = Période
hxj-days = %n jours
hxj-q = Filtrer : titre, valeurs, personne, appareil, n°
hxj-filter = Filtre
hxj-view = Vue
hxj-v-map = Plan des lignes
hxj-v-list = Liste détaillée
hxj-today = Aujourd’hui
hxj-yesterday = Hier
hxj-now = maintenant
hxj-k-event = événement
hxj-k-task = tâche
hxj-k-contact = contact
hxj-kp-event = événements
hxj-kp-task = tâches
hxj-kp-contact = contacts
hxj-bulk-colors = Couleurs appliquées (« %name »)
hxj-bulk-import = Import
hxj-bulk-import-name = Import « %name »
hxj-src-ui = Cette appli
hxj-src-dav = Appareil (synchro)
hxj-src-import = Import
hxj-src-undo = Annulation
hxj-act-create = créé
hxj-act-update = modifié
hxj-act-move = déplacé
hxj-act-delete = supprimé
hxj-act-restore = restauré
hxj-s-undo = %who a annulé %ref
hxj-s-bulk = %who a %act %n %kinds
hxj-s-event = %who a %act l’événement %title
hxj-s-task = %who a %act la tâche %title
hxj-s-contact = %who a %act le contact %title
hxj-undone-short = annulé
hxj-more = { $n ->
    [one] et %n autre
   *[other] et %n autres
}
hxj-empty = (vide)
hxj-st-reverts = annule %ref
hxj-st-undone = annulé par %ref
hxj-st-inactive = inactif · élément supprimé par %ref
hxj-dot-undone = annulé
hxj-dot-inactive = inactif
hxj-why-undone = Déjà annulé (%ref).
hxj-why-inactive = Inactif : l’élément a été supprimé par %ref.
hxj-why-gone = L’élément n’existe plus (%ref) – annulez d’abord la suppression.
hxj-why-exists = L’élément existe déjà à nouveau.
hxj-why-same = L’élément est déjà dans son état d’avant.
hxj-why-coll = Le calendrier ou le carnet est supprimé.
hxj-why-access = Pas de droit d’écriture sur ce calendrier ou carnet.
hxj-undo = Annuler
hxj-undo-warn = Annuler – avec un avertissement
hxj-undo-anyway = Annuler quand même
hxj-cancel = Annuler
hxj-err = Cela n’a pas fonctionné.
hxj-ask = Annuler %ref ?
hxj-done = %ref annulé → nouvelle modification %id.
hxj-w-later = { $n ->
    [one] « %title » sera supprimé. La modification ultérieure (%refs) devient inactive.
   *[other] « %title » sera supprimé. Les %n modifications ultérieures (%refs) deviennent inactives.
}
hxj-w-conflict = « %label » a été modifié depuis%by. L’annulation remplace « %cur » par « %old ».
hxj-w-skip = Ignoré : « %title » – %why
hxj-f-kinds = Type
hxj-f-srcs = Source
hxj-f-who = Personne
hxj-f-acts = Action
hxj-f-colls = Calendrier / carnet
hxj-f-attrs = Attribut modifié
hxj-f-period = Période
hxj-f-status = Statut
hxj-p-all = { $n ->
    [one] Tout (%n jour)
   *[other] Tout (%n jours)
}
hxj-p-today = Aujourd’hui
hxj-p-yesterday = Hier
hxj-p-7 = 7 derniers jours
hxj-p-30 = 30 derniers jours
hxj-p-custom = Période …
hxj-fs-all = Toutes
hxj-fs-active = Actives
hxj-fs-undone = Annulées
hxj-st-only-active = actives seulement
hxj-st-only-undone = annulées/inactives seulement
hxj-only-el = Seulement « %title »
hxj-only-el-tip = N’afficher que cet élément
hxj-with-tech = avec techniques
hxj-show-tech = Afficher les modifications techniques
hxj-reset = Réinitialiser les filtres
hxj-lg-hollow = annulé / inactif
hxj-lg-arc = L’annulation renvoie en arrière
hxj-empty-window = Aucune modification sur cette période.
hxj-before-window = { $n ->
    [one] %n modification correspondante est antérieure –
   *[other] %n modifications correspondantes sont antérieures –
}
hxj-retention = { $n ->
    [one] L’historique remonte à %n jour (jusqu’au %date). Les plus anciennes sont supprimées chaque jour.
   *[other] L’historique remonte à %n jours (jusqu’au %date). Les plus anciennes sont supprimées chaque jour.
}
hxj-close = Fermer
hxj-tech = technique
hxj-col-before = Avant
hxj-col-after = Après
hxj-col-now = Actuel
hxj-col-el = Élément
hxj-present = présent
hxj-new = nouveau
hxj-note-create = Créé avec :
hxj-note-delete = A été supprimé :
hxj-note-restore = A été restauré :
hxj-same-as-after = = Après
hxj-pv-no = Impossible à annuler
hxj-pv-plain = La modification est retirée.
hxj-pv-note = L’annulation est elle-même une modification, annulable à son tour.
hxj-see = voir %ref
hxj-pick = Choisissez une modification à gauche.
hxj-back = Liste
hxj-hist-of = Historique de « %title »
hxj-ruler = Frise
hxj-none = Rien trouvé.

# first start (domain::onboarding): the language gate and the way back to the hints
onb-lang-title = Langue
onb-lang-ask = PractiCal parle la langue de votre navigateur. Est-ce correct ?
onb-lang-go = Continuer
onb-lang-later = Plus tard : Paramètres › Général › Langue
onb-reopen = Premiers pas
onb-reopen-hint = Affiche de nouveau les conseils d'installation. Ce que vous avez déjà confirmé reste confirmé.
err-onboarding-step = Étape d'installation inconnue.
err-lang = Cette langue n'existe pas.

# --- remote import and sync (domain::remote)
calendar-sync = Synchroniser un agenda …
addressbook-sync = Synchroniser un carnet d’adresses …
set-remote-import = Importer depuis Nextcloud
set-remote-sync = Synchroniser avec Nextcloud
rm-server = Adresse du serveur
rm-connect = Se connecter …
rm-connect-hint = La connexion se fait chez Nextcloud même, dans une nouvelle fenêtre. PractiCal ne voit jamais votre mot de passe et ne reçoit qu’un mot de passe d’application, révocable à tout moment dans Nextcloud.
js-rm-waiting = En attente de votre confirmation dans l’autre fenêtre …
rm-waiting = En attente de votre confirmation dans l’autre fenêtre …
rm-scope-hint = Nextcloud donne aussi accès à vos fichiers à un mot de passe d’application. Vous pouvez le désactiver pour ce mot de passe sous « Sécurité » dans Nextcloud.
rm-pick = Que voulez-vous reprendre ?
rm-pick-hint = Plusieurs à la fois, c’est possible. Seuls vos propres agendas et carnets d’adresses sont proposés : Nextcloud ne transmet pas un agenda partagé en entier — les événements privés manquent, les confidentiels se réduisent à « Occupé ».
rm-owner-hint = S’applique à toutes les collections choisies. Si un groupe en est propriétaire, tous ses membres la voient et la modifient.
js-rm-nothing = Il n’y a rien à reprendre ici.
js-rm-readonly-src = lecture seule (Nextcloud n’autorise pas l’écriture)
rm-import-go = Importer une fois
rm-link-go = Mettre en place la synchronisation
rm-direction = Sens
rm-both = Les deux sens
rm-pull = Lecture seule
js-rm-both-hint = Les modifications circulent dans les deux sens. Si les deux côtés modifient le même événement, Nextcloud gagne — votre version reste dans l’historique et peut y être récupérée.
rm-both-hint = Les modifications circulent dans les deux sens. Si les deux côtés modifient le même événement, Nextcloud gagne — votre version reste dans l’historique et peut y être récupérée.
js-rm-pull-hint = Nextcloud est seulement lu. Rien n’y est modifié.
rm-every = Synchroniser toutes les
rm-minutes = minutes
rm-every-hint = Vos propres modifications partent aussitôt. Cet intervalle sert à interroger Nextcloud.
rm-links = Synchronisation
rm-never = pas encore exécutée
rm-now = Synchroniser maintenant
rm-remove = Arrêter la synchronisation
js-rm-remove-hint = La collection et ses entrées restent dans PractiCal. Seule la connexion est coupée et le mot de passe d’application supprimé chez Nextcloud.
rm-remove-hint = La collection et ses entrées restent dans PractiCal. Seule la connexion est coupée et le mot de passe d’application supprimé chez Nextcloud.
rm-ran = Dernière synchronisation
err-remote-not-member = Vous n’êtes plus membre du groupe propriétaire de cette collection. La synchronisation est en pause.
err-remote-kind = Type de collection inconnu.
err-remote-href = Le serveur a renvoyé vers une adresse extérieure.
err-remote-discover = Aucun accès aux agendas n’a été trouvé sur ce serveur.
err-remote-login = La connexion à Nextcloud a échoué.
err-remote-auth = Le mot de passe d’application n’est plus valable. Reconnectez-vous.
err-remote-status = Nextcloud a répondu { $status }.
err-remote-net = Nextcloud était injoignable ({ $what }).
err-remote-busy = Nextcloud demande une pause de { $seconds } secondes.
err-remote-gone = Cette collection n’existe plus sur le serveur.
err-remote-no-key = Une synchronisation durable exige une clé que le serveur n’a pas. Un import unique reste possible.
err-remote-too-many = Pas plus de synchronisations possibles.
err-remote-wait = Vient de s’exécuter — patientez un instant.
err-remote-not-offered = Cette collection n’est plus proposée.
hxj-src-sync = Synchronisation
err-remote-skipped = Certaines entrées n’ont pu être synchronisées. La prochaine ronde réessaiera.
err-remote-cut = La réponse de Nextcloud était incomplète. La prochaine ronde réessaiera.
err-remote-big = La réponse de Nextcloud était trop grande (limite { $limit } octets).
err-remote-already-linked = Cet agenda est déjà synchronisé.
err-remote-unreachable = Nextcloud était injoignable.
js-onb-remote = Saisissez l’adresse, puis connectez-vous.
js-onb-remote-b = L’adresse de votre Nextcloud, p. ex. nextcloud.example.com. La connexion se fait ensuite chez Nextcloud même, dans une nouvelle fenêtre ; PractiCal ne voit jamais votre mot de passe.
js-onb-remote-sync = Saisissez l’adresse, puis connectez-vous.
js-onb-remote-sync-b = Comme l’import, sauf que la liaison reste : les modifications circulent ensuite dans les deux sens. Vous pouvez aussi choisir « lecture seule ».
js-onb-remote-pick-cal = Connecté. Choisissez les agendas et lancez l’import.
js-onb-remote-pick-cal-b = Tous vos agendas sont déjà cochés. Décochez ce qui doit rester, puis cliquez sur « Importer une fois ».
js-onb-remote-pick-ab = Connecté. Choisissez les carnets d’adresses et lancez l’import.
js-onb-remote-pick-ab-b = Tous vos carnets d’adresses sont déjà cochés. Décochez ce qui doit rester, puis cliquez sur « Importer une fois ».
js-onb-remote-sync-pick-cal = Connecté. Choisissez les agendas et mettez en place la synchronisation.
js-onb-remote-sync-pick-cal-b = Tous vos agendas sont déjà cochés. Décochez ce qui ne doit pas être synchronisé, vérifiez le sens et la fréquence, puis cliquez sur « Mettre en place la synchronisation ».
js-onb-remote-sync-pick-ab = Connecté. Choisissez les carnets d’adresses et mettez en place la synchronisation.
js-onb-remote-sync-pick-ab-b = Tous vos carnets d’adresses sont déjà cochés. Décochez ce qui ne doit pas être synchronisé, vérifiez le sens et la fréquence, puis cliquez sur « Mettre en place la synchronisation ».
src-nextcloud = Depuis Nextcloud …
set-sync-calendar = Synchroniser un agenda
set-sync-addressbook = Synchroniser un carnet d’adresses
sync-hint = L’agenda reste relié à l’autre application : les modifications circulent dans les deux sens. Choisissez la source.
imp-choose-vcf = Depuis un fichier .vcf …
js-onb-from = D’où doivent venir les entrées ?
js-onb-from-b = D’un fichier que vous avez exporté, ou directement de Nextcloud. Dans les deux cas, une seule fois, et rien n’est retenu ensuite.
js-onb-sync = Avec quoi synchroniser ?
js-onb-sync-b = La liaison reste : ce que vous changez ici apparaît là-bas, et inversement.
rm-server-ph = nextcloud.example.com
rm-doing = Reprise en cours …
js-rm-doing-title = Reprise en cours …
js-rm-entry-count = { $n ->
    [one] 1 entrée
   *[other] { $n } entrées
  }
js-rm-need-server = Veuillez saisir l’adresse de votre Nextcloud.
err-remote-no-server = Une adresse de serveur est nécessaire.
err-remote-bad-server = Ce n’est pas une adresse de serveur. On attend par exemple nextcloud.example.com.
err-remote-bad-direction = Cette direction n’existe pas.
err-remote-https = L’adresse doit être joignable en https:// — sinon le mot de passe et vos événements circuleraient en clair.
err-remote-private = Cette adresse n’est pas sur l’internet ouvert. Par sécurité le serveur n’atteint que des adresses publiques ; un serveur interne doit être autorisé par l’exploitant.
js-rm-need-pick = Veuillez en choisir au moins un.
js-rm-doing = Récupération de %name … (%n sur %of)
js-rm-took = { $n ->
    [one] %n repris.
   *[other] %n repris.
}
js-rm-failed = { $n ->
    [one] %n non repris.
   *[other] %n non repris.
}
