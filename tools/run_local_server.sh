#!/bin/bash

set -e

SIMPLE_KBS_DIR="$(dirname $0)/.."

export KBS_DB_TYPE=mysql
export KBS_DB_HOST=127.0.0.1
export KBS_DB_USER=root
export KBS_DB_PW=root
#export KBS_DB=simple_kbs
export KBS_DB=simple_kbs

echo "+ Starting kbs-db-mysql DB server container..."
docker run --detach --rm \
           -p 3306:3306 \
           --name kbs-db \
           --env MARIADB_ROOT_PASSWORD=$KBS_DB_PW \
           mariadb:latest

sleep 5
echo -n "+ Trying to connect to mysql DB server..."
while true ; do
  echo -n '.'
  if mysql --silent -u${KBS_DB_USER} -p${KBS_DB_PW} -h ${KBS_DB_HOST} -e "SELECT 1;" > /dev/null 2>&1 ; then
    echo " Success!"
    break
  fi
  sleep 1
done

echo "+ Creating mysql database ${KBS_DB}"
mysql -u${KBS_DB_USER} -p${KBS_DB_PW} -h ${KBS_DB_HOST} -e "CREATE DATABASE ${KBS_DB};"
mysql -u${KBS_DB_USER} -p${KBS_DB_PW} -h ${KBS_DB_HOST} ${KBS_DB} < "${SIMPLE_KBS_DIR}/db-mysql.sql"

RUST_LOG=debug cargo run

