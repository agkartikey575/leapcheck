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
 * @param array $item Inventory item.
 * @return string
 */
function leapcheck_result_url( array $item ) {
	return leapcheck_base_url() . '/v1/' . $item['ecosystem'] . '/' . rawurlencode( $item['slug'] ) . '/' . rawurlencode( $item['version'] ) . '.json';
}

/**
 * Transient key for an item (max 172 chars; md5 keeps it short).
 *
 * @param array $item Inventory item.
 * @return string
 */
function leapcheck_transient_key( array $item ) {
	return 'leapcheck_r_' . md5( leapcheck_result_url( $item ) );
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
 * Get the result record for an item: from cache, or (only with consent) by a plain GET.
 *
 * Record: array( 'status' => 'ok'|'missing'|'error', 'data' => parsed|null, 'fetched' => int ).
 *
 * @param array $item Inventory item.
 * @return array
 */
function leapcheck_get_result( array $item ) {
	static $offline = false;

	if ( ! $item['valid'] ) {
		return array(
			'status'  => 'missing',
			'data'    => null,
			'fetched' => 0,
		);
	}

	$key    = leapcheck_transient_key( $item );
	$cached = get_transient( $key );
	if ( is_array( $cached ) && isset( $cached['status'] ) ) {
		return $cached;
	}

	// Guideline 7: never contact the service without the site owner's opt-in.
	// After one network failure, skip the rest of this request instead of waiting on each item.
	if ( ! leapcheck_has_consent() || $offline ) {
		return array(
			'status'  => 'error',
			'data'    => null,
			'fetched' => 0,
		);
	}

	// Plain GET of a public file. A neutral user agent replaces WordPress's default one,
	// which would include this site's URL; no other headers or site data are sent.
	$response = wp_remote_get(
		leapcheck_result_url( $item ),
		array(
			'timeout'             => 5,
			'redirection'         => 2,
			'user-agent'          => 'LeapCheck/' . LEAPCHECK_VERSION,
			'limit_response_size' => 2 * 1024 * 1024,
			'headers'             => array( 'Accept' => 'application/json' ),
		)
	);

	$record = array(
		'status'  => 'error',
		'data'    => null,
		'fetched' => time(),
	);
	$ttl    = 15 * MINUTE_IN_SECONDS;

	if ( is_wp_error( $response ) ) {
		$offline = true;
	} else {
		$code = (int) wp_remote_retrieve_response_code( $response );
		if ( 200 === $code ) {
			$data = leapcheck_parse_result( json_decode( wp_remote_retrieve_body( $response ), true ) );
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
		$total    = 0;
		if ( isset( $target['findings'] ) && is_array( $target['findings'] ) ) {
			foreach ( $target['findings'] as $finding ) {
				if ( ! is_array( $finding ) ) {
					continue;
				}
				++$total;
				if ( count( $findings ) >= LEAPCHECK_MAX_FINDINGS ) {
					continue;
				}
				$severity   = leapcheck_str( $finding, 'severity' );
				$findings[] = array(
					'rule_id'    => leapcheck_str( $finding, 'rule_id' ),
					'severity'   => in_array( $severity, array( 'error', 'warning' ), true ) ? $severity : 'warning',
					'confidence' => leapcheck_str( $finding, 'confidence' ),
					'message'    => leapcheck_str( $finding, 'message' ),
					'file'       => leapcheck_str( $finding, 'file' ),
					'line'       => leapcheck_int( $finding, 'line' ),
					'component'  => leapcheck_str( $finding, 'component' ),
				);
			}
		}
		$verdict                = leapcheck_str( $target, 'verdict' );
		$targets[ $target_key ] = array(
			'verdict'        => in_array( $verdict, $verdicts, true ) ? $verdict : 'unknown',
			'errors'         => leapcheck_int( $target, 'errors' ),
			'warnings'       => leapcheck_int( $target, 'warnings' ),
			'findings'       => $findings,
			'findings_total' => $total,
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

	return array(
		'name'           => leapcheck_str( $data, 'name' ),
		'version'        => leapcheck_str( $data, 'version' ),
		'scanned_at'     => leapcheck_str( $data, 'scanned_at' ),
		'engine_version' => leapcheck_str( $data, 'engine_version' ),
		'targets'        => $targets,
		'components'     => $components,
	);
}

/**
 * Verdict of one item for one PHP version. Pure function.
 *
 * @param array  $record Result record from leapcheck_get_result().
 * @param string $target PHP version, e.g. '8.4'.
 * @return string compatible|issues|breaks|unknown|missing|error
 */
function leapcheck_item_verdict( array $record, $target ) {
	if ( ! isset( $record['status'] ) || 'ok' !== $record['status'] ) {
		return ( isset( $record['status'] ) && 'missing' === $record['status'] ) ? 'missing' : 'error';
	}
	$key = 'php:' . $target;
	return isset( $record['data']['targets'][ $key ]['verdict'] ) ? $record['data']['targets'][ $key ]['verdict'] : 'unknown';
}

/**
 * Count verdicts of the items in use (active plugins, active and parent theme). Pure function.
 *
 * @param array[] $rows   List of array( 'item' => item, 'record' => record ).
 * @param string  $target PHP version.
 * @return array{compatible:int,issues:int,breaks:int,unchecked:int,total:int,state:string}
 */
function leapcheck_summarize( array $rows, $target ) {
	$counts = array(
		'compatible' => 0,
		'issues'     => 0,
		'breaks'     => 0,
		'unchecked'  => 0,
		'total'      => 0,
	);
	foreach ( $rows as $row ) {
		if ( empty( $row['item']['active'] ) ) {
			continue;
		}
		++$counts['total'];
		$verdict = leapcheck_item_verdict( $row['record'], $target );
		if ( isset( $counts[ $verdict ] ) && 'total' !== $verdict && 'unchecked' !== $verdict ) {
			++$counts[ $verdict ];
		} else {
			++$counts['unchecked'];
		}
	}

	if ( 0 === $counts['total'] ) {
		$state = 'empty';
	} elseif ( $counts['breaks'] > 0 ) {
		$state = 'breaks';
	} elseif ( $counts['issues'] > 0 ) {
		$state = 'issues';
	} elseif ( $counts['unchecked'] > 0 ) {
		$state = 'unchecked';
	} else {
		$state = 'compatible';
	}
	$counts['state'] = $state;
	return $counts;
}
