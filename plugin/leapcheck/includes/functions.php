<?php
/**
 * Core logic: inventory, fetching and parsing result files, verdicts.
 *
 * Nothing in this file runs on load; it only defines functions.
 * Result file format: docs/07-interfaces.md ("Static result files").
 *
 * @package LeapCheck
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

/** Option that stores the site owner's opt-in (array with `time`). */
const LEAPCHECK_OPTION_CONSENT = 'leapcheck_consent';

/** Option that lists every transient key we created, so uninstall can remove them. */
const LEAPCHECK_OPTION_CACHE_KEYS = 'leapcheck_cache_keys';

/** Max findings kept per target per item (keeps transients small). */
const LEAPCHECK_MAX_FINDINGS = 200;

/** Max remote fetches per page load; the rest continue on the next load ("Check remaining"). */
const LEAPCHECK_MAX_FETCHES = 25;

/** Characters trimmed from strings; explicit so behaviour is identical on every PHP version. */
const LEAPCHECK_TRIM_CHARS = " \t\n\r\0\x0B\f";

/**
 * PHP versions we report on (phase 1 targets).
 *
 * @return string[]
 */
function leapcheck_targets() {
	return array( '8.3', '8.4', '8.5' );
}

/**
 * Has the site owner opted in to fetching results?
 *
 * @return bool
 */
function leapcheck_has_consent() {
	$consent = get_option( LEAPCHECK_OPTION_CONSENT );
	return is_array( $consent ) && ! empty( $consent['time'] );
}

/**
 * Base URL of the static result files, without trailing slash.
 *
 * @return string
 */
function leapcheck_base_url() {
	/**
	 * Filters the base URL of the LeapCheck result files.
	 *
	 * @param string $url Default LEAPCHECK_BASE_URL.
	 */
	$url = apply_filters( 'leapcheck_base_url', LEAPCHECK_BASE_URL );
	$url = esc_url_raw( (string) $url, array( 'https', 'http' ) );
	if ( '' === $url ) {
		$url = LEAPCHECK_BASE_URL;
	}
	return untrailingslashit( $url );
}

/**
 * Is a slug or version safe to put in a URL path segment?
 *
 * @param mixed $value Value to check.
 * @return bool
 */
function leapcheck_is_valid_part( $value ) {
	return is_string( $value )
		&& strlen( $value ) <= 200
		&& 1 === preg_match( '/^[A-Za-z0-9][A-Za-z0-9._+-]*$/', $value );
}

/**
 * Build one inventory item.
 *
 * @param string $kind    'plugin' or 'theme'.
 * @param string $slug    WordPress.org slug (folder name).
 * @param string $name    Display name.
 * @param string $version Installed version.
 * @param bool   $active  Whether it is in use.
 * @param string $role    '', 'active-theme' or 'parent-theme'.
 * @return array
 */
function leapcheck_make_item( $kind, $slug, $name, $version, $active, $role = '' ) {
	$slug    = strtolower( trim( (string) $slug, LEAPCHECK_TRIM_CHARS ) );
	$version = trim( (string) $version, LEAPCHECK_TRIM_CHARS );
	return array(
		'kind'      => $kind,
		'ecosystem' => ( 'theme' === $kind ) ? 'wordpress-theme' : 'wordpress-plugin',
		'slug'      => $slug,
		'name'      => '' !== trim( (string) $name, LEAPCHECK_TRIM_CHARS ) ? (string) $name : $slug,
		'version'   => $version,
		'active'    => (bool) $active,
		'role'      => $role,
		'valid'     => leapcheck_is_valid_part( $slug ) && leapcheck_is_valid_part( $version ),
	);
}

/**
 * Installed plugins plus the active theme and its parent.
 *
 * @return array[]
 */
function leapcheck_get_items() {
	if ( ! function_exists( 'get_plugins' ) ) {
		require_once ABSPATH . 'wp-admin/includes/plugin.php';
	}

	$items = array();
	foreach ( get_plugins() as $file => $data ) {
		$dir  = dirname( $file );
		$slug = ( '.' === $dir ) ? basename( $file, '.php' ) : $dir;
		$items[] = leapcheck_make_item(
			'plugin',
			$slug,
			isset( $data['Name'] ) ? $data['Name'] : '',
			isset( $data['Version'] ) ? $data['Version'] : '',
			is_plugin_active( $file )
		);
	}

	$theme = wp_get_theme();
	if ( $theme->exists() ) {
		$items[] = leapcheck_make_item( 'theme', $theme->get_stylesheet(), $theme->get( 'Name' ), $theme->get( 'Version' ), true, 'active-theme' );
		$parent  = $theme->parent();
		if ( $parent && $parent->exists() ) {
			$items[] = leapcheck_make_item( 'theme', $parent->get_stylesheet(), $parent->get( 'Name' ), $parent->get( 'Version' ), true, 'parent-theme' );
		}
	}

	return $items;
}

/**
 * Public URL of an item's result file.
 *
 * @param array       $item    Inventory item.
 * @param string|null $version Version to look up; defaults to the installed version.
 * @return string
 */
function leapcheck_result_url( array $item, $version = null ) {
	$version = ( null === $version ) ? $item['version'] : (string) $version;
	return leapcheck_base_url() . '/v1/' . $item['ecosystem'] . '/' . rawurlencode( $item['slug'] ) . '/' . rawurlencode( $version ) . '.json';
}

/**
 * Public URL of an item's index file (lists the scanned versions).
 *
 * @param array $item Inventory item.
 * @return string
 */
function leapcheck_index_url( array $item ) {
	return leapcheck_base_url() . '/v1/' . $item['ecosystem'] . '/' . rawurlencode( $item['slug'] ) . '/index.json';
}

/**
 * Transient key for a URL (max 172 chars; md5 keeps it short).
 * The "r2" prefix keeps records cached by older plugin versions (different shape) from being read.
 *
 * @param string $url File URL.
 * @return string
 */
function leapcheck_cache_key( $url ) {
	return 'leapcheck_r2_' . md5( (string) $url );
}

/**
 * Store a record in a transient and remember its key for cleanup.
 *
 * @param string $key    Transient key.
 * @param array  $record Record.
 * @param int    $ttl    Seconds.
 */
function leapcheck_cache_set( $key, array $record, $ttl ) {
	set_transient( $key, $record, $ttl );
	$keys = get_option( LEAPCHECK_OPTION_CACHE_KEYS, array() );
	if ( ! is_array( $keys ) ) {
		$keys = array();
	}
	if ( ! in_array( $key, $keys, true ) ) {
		$keys[] = $key;
		update_option( LEAPCHECK_OPTION_CACHE_KEYS, $keys, false );
	}
}

/**
 * Delete every cached result.
 */
function leapcheck_cache_clear() {
	$keys = get_option( LEAPCHECK_OPTION_CACHE_KEYS, array() );
	if ( is_array( $keys ) ) {
		foreach ( $keys as $key ) {
			delete_transient( (string) $key );
		}
	}
	delete_option( LEAPCHECK_OPTION_CACHE_KEYS );
}

/**
 * Network state for the current page load: remote fetches used, and whether the service was unreachable.
 *
 * @param string $op 'get', 'use' (count one fetch), 'offline' (mark unreachable) or 'reset'.
 * @return array{used:int,offline:bool}
 */
function leapcheck_fetch_state( $op = 'get' ) {
	static $state = array(
		'used'    => 0,
		'offline' => false,
	);
	if ( 'reset' === $op ) {
		$state = array(
			'used'    => 0,
			'offline' => false,
		);
	} elseif ( 'use' === $op ) {
		++$state['used'];
	} elseif ( 'offline' === $op ) {
		$state['offline'] = true;
	}
	return $state;
}

/**
 * A result record without data.
 *
 * @param string $status ok|missing|error|pending.
 * @param int    $fetched Time of the fetch, 0 if none.
 * @return array
 */
function leapcheck_record( $status, $fetched = 0 ) {
	return array(
		'status'   => $status,
		'data'     => null,
		'fetched'  => $fetched,
		'fallback' => '',
	);
}

/**
 * Get one public JSON file: from cache, or (only with consent, and within the per-page fetch limit) by a plain GET.
 *
 * Record: array( 'status' => 'ok'|'missing'|'error'|'pending', 'data' => parsed|null, 'fetched' => int, 'fallback' => '' ).
 * 'pending' means the per-page fetch limit was reached; nothing is cached, so the next page load continues.
 *
 * @param string   $url    File URL.
 * @param callable $parser Turns decoded JSON into normalised data, or null when invalid.
 * @return array
 */
function leapcheck_fetch_json( $url, $parser ) {
	$key    = leapcheck_cache_key( $url );
	$cached = get_transient( $key );
	if ( is_array( $cached ) && isset( $cached['status'] ) ) {
		return $cached;
	}

	// Guideline 7: never contact the service without the site owner's opt-in.
	// After one network failure, skip the rest of this request instead of waiting on each item.
	$state = leapcheck_fetch_state();
	if ( ! leapcheck_has_consent() || $state['offline'] ) {
		return leapcheck_record( 'error' );
	}
	if ( $state['used'] >= LEAPCHECK_MAX_FETCHES ) {
		return leapcheck_record( 'pending' );
	}
	leapcheck_fetch_state( 'use' );

	// Plain GET of a public file. A neutral user agent replaces WordPress's default one,
	// which would include this site's URL; no other headers or site data are sent.
	$response = wp_remote_get(
		$url,
		array(
			'timeout'             => 5,
			'redirection'         => 2,
			'user-agent'          => 'LeapCheck/' . LEAPCHECK_VERSION,
			'limit_response_size' => 2 * 1024 * 1024,
			'headers'             => array( 'Accept' => 'application/json' ),
		)
	);

	$record = leapcheck_record( 'error', time() );
	$ttl    = 15 * MINUTE_IN_SECONDS;

	if ( is_wp_error( $response ) ) {
		leapcheck_fetch_state( 'offline' );
	} else {
		$code = (int) wp_remote_retrieve_response_code( $response );
		if ( 200 === $code ) {
			$data = call_user_func( $parser, json_decode( wp_remote_retrieve_body( $response ), true ) );
			if ( null !== $data ) {
				$record['status'] = 'ok';
				$record['data']   = $data;
				$ttl              = 12 * HOUR_IN_SECONDS;
			}
		} elseif ( 404 === $code ) {
			$record['status'] = 'missing';
			$ttl              = 12 * HOUR_IN_SECONDS;
		}
	}

	leapcheck_cache_set( $key, $record, $ttl );
	return $record;
}

/**
 * Get the result record for an item.
 *
 * If the installed version was not scanned, the item's index file is read and the closest scanned
 * version's result is returned instead, with `fallback` set to that version.
 *
 * @param array $item Inventory item.
 * @return array Record, see leapcheck_fetch_json().
 */
function leapcheck_get_result( array $item ) {
	if ( ! $item['valid'] ) {
		return leapcheck_record( 'missing' );
	}

	$main = leapcheck_fetch_json( leapcheck_result_url( $item ), 'leapcheck_parse_result' );
	if ( 'missing' !== $main['status'] ) {
		return $main;
	}

	$index = leapcheck_fetch_json( leapcheck_index_url( $item ), 'leapcheck_parse_index' );
	if ( 'ok' !== $index['status'] ) {
		return ( 'missing' === $index['status'] ) ? $main : leapcheck_record( $index['status'], $index['fetched'] );
	}

	$closest = leapcheck_closest_version( $index['data']['versions'], $item['version'] );
	if ( '' === $closest ) {
		return $main;
	}

	$alt = leapcheck_fetch_json( leapcheck_result_url( $item, $closest ), 'leapcheck_parse_result' );
	if ( 'ok' !== $alt['status'] ) {
		return ( 'missing' === $alt['status'] ) ? $main : leapcheck_record( $alt['status'], $alt['fetched'] );
	}
	$alt['fallback'] = $closest;
	return $alt;
}

/**
 * Validate and normalise a decoded index file. Pure function.
 *
 * @param mixed $data Decoded JSON.
 * @return array|null array( 'versions' => string[], 'latest' => string ), or null when invalid.
 */
function leapcheck_parse_index( $data ) {
	if ( ! is_array( $data ) || ! isset( $data['schema'] ) || 1 !== (int) $data['schema']
		|| ! isset( $data['versions'] ) || ! is_array( $data['versions'] ) ) {
		return null;
	}
	$versions = array();
	foreach ( $data['versions'] as $version ) {
		if ( leapcheck_is_valid_part( $version ) && ! in_array( $version, $versions, true ) ) {
			$versions[] = $version;
		}
		if ( count( $versions ) >= 500 ) {
			break;
		}
	}
	$latest = ( isset( $data['latest'] ) && leapcheck_is_valid_part( $data['latest'] ) ) ? $data['latest'] : '';
	return array(
		'versions' => $versions,
		'latest'   => $latest,
	);
}

/**
 * Pick the scanned version closest to the installed one. Pure function.
 *
 * "Closest" = shares the most leading version parts (5.1.2 is closer to 5.1.9 than to 5.0);
 * on a tie, the nearest older version wins, then the nearest newer one.
 *
 * @param string[] $versions  Scanned versions.
 * @param string   $installed Installed version.
 * @return string Version, or '' when there is no other version.
 */
function leapcheck_closest_version( array $versions, $installed ) {
	$installed = (string) $installed;
	$parts     = explode( '.', $installed );
	$best      = '';
	$best_rank = null;
	foreach ( $versions as $version ) {
		if ( ! is_string( $version ) || $version === $installed ) {
			continue;
		}
		$common = 0;
		foreach ( explode( '.', $version ) as $i => $part ) {
			if ( ! isset( $parts[ $i ] ) || $parts[ $i ] !== $part ) {
				break;
			}
			++$common;
		}
		$older = version_compare( $version, $installed, '<' ) ? 1 : 0;
		$rank  = array( $common, $older );
		if ( null === $best_rank || $rank > $best_rank
			|| ( $rank === $best_rank && ( $older ? version_compare( $version, $best, '>' ) : version_compare( $version, $best, '<' ) ) ) ) {
			$best      = $version;
			$best_rank = $rank;
		}
	}
	return $best;
}

/**
 * Read a scalar field as a trimmed, length-limited string.
 *
 * @param array  $arr Source.
 * @param string $key Field.
 * @return string
 */
function leapcheck_str( array $arr, $key ) {
	if ( ! isset( $arr[ $key ] ) || ! is_scalar( $arr[ $key ] ) ) {
		return '';
	}
	return substr( trim( (string) $arr[ $key ], LEAPCHECK_TRIM_CHARS ), 0, 500 );
}

/**
 * Read a field as a non-negative integer.
 *
 * @param array  $arr Source.
 * @param string $key Field.
 * @return int
 */
function leapcheck_int( array $arr, $key ) {
	return ( isset( $arr[ $key ] ) && is_numeric( $arr[ $key ] ) ) ? max( 0, (int) $arr[ $key ] ) : 0;
}

/**
 * Validate and normalise a decoded result file (schema 1). Pure function.
 *
 * @param mixed $data Decoded JSON.
 * @return array|null Null when the file is not a valid schema-1 result.
 */
function leapcheck_parse_result( $data ) {
	if ( ! is_array( $data ) || ! isset( $data['schema'] ) || 1 !== (int) $data['schema']
		|| ! isset( $data['targets'] ) || ! is_array( $data['targets'] ) ) {
		return null;
	}

	$verdicts = array( 'compatible', 'issues', 'breaks', 'unknown' );
	$targets  = array();

	foreach ( $data['targets'] as $target_key => $target ) {
		if ( ! is_string( $target_key ) || ! is_array( $target ) ) {
			continue;
		}
		$findings = array();
		$guarded  = array();
		$total    = 0;
		$info     = 0;
		if ( isset( $target['findings'] ) && is_array( $target['findings'] ) ) {
			foreach ( $target['findings'] as $finding ) {
				if ( ! is_array( $finding ) ) {
					continue;
				}
				$severity = leapcheck_str( $finding, 'severity' );
				if ( ! in_array( $severity, array( 'error', 'warning', 'info' ), true ) ) {
					$severity = 'warning';
				}
				// "info" = guarded legacy code that is skipped on newer PHP: kept apart and never counted.
				if ( 'info' === $severity ) {
					++$info;
				} else {
					++$total;
				}
				$list_count = ( 'info' === $severity ) ? count( $guarded ) : count( $findings );
				if ( $list_count >= LEAPCHECK_MAX_FINDINGS ) {
					continue;
				}
				$entry = array(
					'rule_id'    => leapcheck_str( $finding, 'rule_id' ),
					'severity'   => $severity,
					'confidence' => leapcheck_str( $finding, 'confidence' ),
					'message'    => leapcheck_str( $finding, 'message' ),
					'file'       => leapcheck_str( $finding, 'file' ),
					'line'       => leapcheck_int( $finding, 'line' ),
					'component'  => leapcheck_str( $finding, 'component' ),
				);
				if ( 'info' === $severity ) {
					$guarded[] = $entry;
				} else {
					$findings[] = $entry;
				}
			}
		}
		$verdict                = leapcheck_str( $target, 'verdict' );
		$targets[ $target_key ] = array(
			'verdict'        => in_array( $verdict, $verdicts, true ) ? $verdict : 'unknown',
			'errors'         => leapcheck_int( $target, 'errors' ),
			'warnings'       => leapcheck_int( $target, 'warnings' ),
			'findings'       => $findings,
			'findings_total' => $total,
			'guarded'        => $guarded,
			'guarded_total'  => $info,
		);
	}

	$components = array();
	if ( isset( $data['components'] ) && is_array( $data['components'] ) ) {
		foreach ( $data['components'] as $component ) {
			if ( ! is_array( $component ) || '' === leapcheck_str( $component, 'name' ) ) {
				continue;
			}
			$components[] = array(
				'name'                  => leapcheck_str( $component, 'name' ),
				'version'               => leapcheck_str( $component, 'version' ),
				'kind'                  => leapcheck_str( $component, 'kind' ),
				'path'                  => leapcheck_str( $component, 'path' ),
				'detection'             => leapcheck_str( $component, 'detection' ),
				'known_vulnerabilities' => leapcheck_int( $component, 'known_vulnerabilities' ),
			);
		}
	}

	$requirements = array();
	if ( isset( $data['requirements'] ) && is_array( $data['requirements'] ) ) {
		foreach ( $data['requirements'] as $requirement ) {
			$ext = is_array( $requirement ) ? strtolower( leapcheck_str( $requirement, 'name' ) ) : '';
			if ( 1 === preg_match( '/^[a-z0-9_]{1,64}$/', $ext ) && ! in_array( $ext, $requirements, true ) ) {
				$requirements[] = $ext;
			}
		}
		sort( $requirements );
	}

	return array(
		'name'            => leapcheck_str( $data, 'name' ),
		'display_name'    => leapcheck_str( $data, 'display_name' ),
		'active_installs' => leapcheck_int( $data, 'active_installs' ),
		'version'         => leapcheck_str( $data, 'version' ),
		'scanned_at'      => leapcheck_str( $data, 'scanned_at' ),
		'engine_version'  => leapcheck_str( $data, 'engine_version' ),
		'targets'         => $targets,
		'components'      => $components,
		'requirements'    => $requirements,
	);
}

/**
 * Verdict of one item for one PHP version. Pure function.
 *
 * @param array  $record Result record from leapcheck_get_result().
 * @param string $target PHP version, e.g. '8.4'.
 * @return string compatible|issues|breaks|unknown|missing|pending|error
 */
function leapcheck_item_verdict( array $record, $target ) {
	if ( ! isset( $record['status'] ) || 'ok' !== $record['status'] ) {
		$status = isset( $record['status'] ) ? $record['status'] : '';
		return in_array( $status, array( 'missing', 'pending' ), true ) ? $status : 'error';
	}
	$key = 'php:' . $target;
	return isset( $record['data']['targets'][ $key ]['verdict'] ) ? $record['data']['targets'][ $key ]['verdict'] : 'unknown';
}

/**
 * Count verdicts of the items in use (active plugins, active and parent theme). Pure function.
 *
 * A result for a different (closest) version is shown per item but counted as "unchecked" here
 * (and in `approx`): it is a strong hint, not an answer for the installed code.
 *
 * @param array[] $rows   List of array( 'item' => item, 'record' => record ).
 * @param string  $target PHP version.
 * @return array{compatible:int,issues:int,breaks:int,unchecked:int,approx:int,total:int,state:string}
 */
function leapcheck_summarize( array $rows, $target ) {
	$counts = array(
		'compatible' => 0,
		'issues'     => 0,
		'breaks'     => 0,
		'unchecked'  => 0,
		'approx'     => 0,
		'total'      => 0,
	);
	foreach ( $rows as $row ) {
		if ( empty( $row['item']['active'] ) ) {
			continue;
		}
		++$counts['total'];
		if ( ! empty( $row['record']['fallback'] ) ) {
			++$counts['approx'];
			++$counts['unchecked'];
			continue;
		}
		$verdict = leapcheck_item_verdict( $row['record'], $target );
		if ( in_array( $verdict, array( 'compatible', 'issues', 'breaks' ), true ) ) {
			++$counts[ $verdict ];
		} else {
			++$counts['unchecked'];
		}
	}
	if ( 0 === $counts['total'] ) {
		$state = 'empty';
	} elseif ( $counts['breaks'] > 0 ) {
		$state = 'breaks';
	} elseif ( $counts['unchecked'] > 0 ) {
		// Never let a "yes, with warnings" hide items we could not check.
		$state = 'unchecked';
	} elseif ( $counts['issues'] > 0 ) {
		$state = 'issues';
	} else {
		$state = 'compatible';
	}
	$counts['state'] = $state;
	return $counts;
}

/**
 * Rows of items in use whose result loaded. Pure function.
 *
 * @param array[] $rows        Items with records.
 * @param bool    $allow_approx Include results for a different (closest) version.
 * @return array[]
 */
function leapcheck_loaded_rows( array $rows, $allow_approx ) {
	$out = array();
	foreach ( $rows as $row ) {
		if ( empty( $row['item']['active'] ) || ! isset( $row['record']['status'] ) || 'ok' !== $row['record']['status']
			|| ! is_array( $row['record']['data'] ) ) {
			continue;
		}
		if ( ! $allow_approx && ! empty( $row['record']['fallback'] ) ) {
			continue;
		}
		$out[] = $row;
	}
	return $out;
}

/**
 * PHP extensions needed by the items in use, sorted and unique. Pure function.
 *
 * @param array[] $rows Items with records.
 * @return string[]
 */
function leapcheck_collect_requirements( array $rows ) {
	$names = array();
	foreach ( leapcheck_loaded_rows( $rows, true ) as $row ) {
		if ( empty( $row['record']['data']['requirements'] ) ) {
			continue;
		}
		foreach ( $row['record']['data']['requirements'] as $ext ) {
			$names[ $ext ] = true;
		}
	}
	$names = array_keys( $names );
	sort( $names );
	return $names;
}

/**
 * Bundled PHP libraries that two or more items in use ship in different versions. Pure function.
 *
 * PHP can load only one copy of a class, so whichever copy loads first is used by every plugin.
 * Only exact results (not closest-version fallbacks) and copies with a known version are compared.
 *
 * @param array[] $rows Items with records.
 * @return array[] List of array( 'library' => name, 'copies' => array( array( 'item', 'version', 'path' ) ) ),
 *                 copies sorted from oldest to newest version.
 */
function leapcheck_find_conflicts( array $rows ) {
	$by_lib = array();
	$labels = array();
	foreach ( leapcheck_loaded_rows( $rows, false ) as $row ) {
		$seen = array();
		foreach ( $row['record']['data']['components'] as $component ) {
			if ( 'php' !== $component['kind'] || '' === $component['version'] ) {
				continue;
			}
			$lib = strtolower( $component['name'] );
			if ( ! isset( $labels[ $lib ] ) ) {
				$labels[ $lib ] = $component['name'];
			}
			// One entry per item and version, even if an item ships the same copy twice.
			$seen_key = $lib . '@' . $component['version'];
			if ( isset( $seen[ $seen_key ] ) ) {
				continue;
			}
			$seen[ $seen_key ] = true;
			$by_lib[ $lib ][]  = array(
				'item'    => $row['item']['name'],
				'slug'    => $row['item']['slug'],
				'version' => $component['version'],
				'path'    => $component['path'],
			);
		}
	}

	$conflicts = array();
	foreach ( $by_lib as $lib => $copies ) {
		$items    = array_unique( array_column( $copies, 'slug' ) );
		$versions = array_unique( array_column( $copies, 'version' ) );
		if ( count( $items ) < 2 || count( $versions ) < 2 ) {
			continue;
		}
		usort(
			$copies,
			function ( $a, $b ) {
				$cmp = version_compare( $a['version'], $b['version'] );
				return 0 !== $cmp ? $cmp : strcmp( $a['item'], $b['item'] );
			}
		);
		$conflicts[] = array(
			'library' => $labels[ $lib ],
			'copies'  => $copies,
		);
	}
	usort(
		$conflicts,
		function ( $a, $b ) {
			return strcmp( $a['library'], $b['library'] );
		}
	);
	return $conflicts;
}
